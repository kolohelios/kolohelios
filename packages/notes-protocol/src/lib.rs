// Non-test code must not `.unwrap()`; `not(test)` exempts unit tests,
// and integration tests compile as separate crates (no attribute).
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

//! Wire protocol shared by the `notes-sync` Durable Object and the WASM
//! editor — one set of serde structs so both sides serialize the same
//! bytes. Compiles natively (for `cargo test`) and to
//! `wasm32-unknown-unknown` (the Worker and the editor).
//!
//! - Client → server: [`ClientMsg::Open`], [`ClientMsg::Edit`].
//! - Server → client: [`ServerMsg::Ack`], [`ServerMsg::Sync`],
//!   [`ServerMsg::BackedUp`].
//!
//! Messages are tagged enums (`{"type":"open",…}`); a [`Delta`] is the
//! seam where a CRDT update type could slot in later without disturbing
//! the surrounding protocol.

use serde::{Deserialize, Serialize};

/// Monotonic per-note sequence number. The Durable Object assigns one to
/// every accepted edit; the client echoes the last seq it has seen so a
/// reconnect can resync from exactly that point. `0` means "no edits
/// seen yet" (a fresh client).
pub type Seq = u64;

/// A change to the note body. `Splice` is the small-delta common case;
/// `Whole` is the always-correct fallback (and the bootstrap shape for a
/// fresh document). This enum is the seam where a CRDT update type could
/// slot in later without changing the surrounding messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Delta {
    /// Replace the byte range `[at, at + remove)` with `insert`. Offsets
    /// are byte offsets into the current UTF-8 text.
    Splice {
        at: usize,
        remove: usize,
        insert: String,
    },
    /// Replace the entire body — the whole-body fallback.
    Whole { text: String },
}

/// Why a [`Delta`] could not be applied to a given text. The caller's
/// recovery is to request a full [`ServerMsg::Sync`] rather than guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyError {
    /// `at + remove` ran past the end of the text.
    OutOfRange {
        at: usize,
        remove: usize,
        len: usize,
    },
    /// An offset landed in the middle of a multi-byte UTF-8 character.
    NotCharBoundary { offset: usize },
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApplyError::OutOfRange { at, remove, len } => write!(
                f,
                "splice [{at}, {at}+{remove}) is out of range for text of length {len}"
            ),
            ApplyError::NotCharBoundary { offset } => {
                write!(f, "offset {offset} is not a UTF-8 char boundary")
            }
        }
    }
}

impl std::error::Error for ApplyError {}

impl Delta {
    /// Apply this delta to `text`, returning the new body. Pure and
    /// native-testable — the Durable Object folds accepted deltas into
    /// the current text by replaying its log through this.
    pub fn apply(&self, text: &str) -> Result<String, ApplyError> {
        match self {
            Delta::Whole { text: replacement } => Ok(replacement.clone()),
            Delta::Splice { at, remove, insert } => {
                let end = at.checked_add(*remove).filter(|e| *e <= text.len()).ok_or(
                    ApplyError::OutOfRange {
                        at: *at,
                        remove: *remove,
                        len: text.len(),
                    },
                )?;
                if !text.is_char_boundary(*at) {
                    return Err(ApplyError::NotCharBoundary { offset: *at });
                }
                if !text.is_char_boundary(end) {
                    return Err(ApplyError::NotCharBoundary { offset: end });
                }
                let mut out = String::with_capacity(text.len() - remove + insert.len());
                out.push_str(&text[..*at]);
                out.push_str(insert);
                out.push_str(&text[end..]);
                Ok(out)
            }
        }
    }
}

/// A message from the editor to the Durable Object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// Open the note and resync from `since_seq` (`0` for a fresh
    /// client). The server replies with a [`ServerMsg::Sync`] carrying
    /// the current seq and text.
    Open { since_seq: Seq },
    /// Apply `delta` on top of `base_seq`. If `base_seq` is not the
    /// server's current seq the edit is stale and is rejected with a
    /// fresh [`ServerMsg::Sync`] instead of an [`ServerMsg::Ack`].
    Edit { base_seq: Seq, delta: Delta },
}

/// A message from the Durable Object to the editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    /// An edit was accepted; `seq` is its assigned sequence number.
    Ack { seq: Seq },
    /// Full state: adopt `text` at `seq`. Sent on open, on a stale edit,
    /// or whenever the server needs to resync the client.
    Sync { seq: Seq, text: String },
    /// The note was committed to git at `commit_sha` (absent when the
    /// commit is still pending or coalesced).
    BackedUp { commit_sha: Option<String> },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splice_replaces_the_byte_range() {
        // "hello world": replace " world" (6 bytes at offset 5) with
        // "there".
        let d = Delta::Splice {
            at: 5,
            remove: 6,
            insert: "there".into(),
        };
        assert_eq!(d.apply("hello world").unwrap(), "hellothere");
    }

    #[test]
    fn splice_with_zero_remove_is_an_insert() {
        let d = Delta::Splice {
            at: 0,
            remove: 0,
            insert: "> ".into(),
        };
        assert_eq!(d.apply("quote").unwrap(), "> quote");
    }

    #[test]
    fn whole_replaces_everything() {
        let d = Delta::Whole {
            text: "fresh".into(),
        };
        assert_eq!(d.apply("anything at all").unwrap(), "fresh");
    }

    #[test]
    fn splice_past_end_is_out_of_range() {
        let d = Delta::Splice {
            at: 3,
            remove: 10,
            insert: String::new(),
        };
        assert_eq!(
            d.apply("hi").unwrap_err(),
            ApplyError::OutOfRange {
                at: 3,
                remove: 10,
                len: 2
            }
        );
    }

    #[test]
    fn splice_off_a_char_boundary_is_rejected() {
        // "é" is two bytes; offset 1 splits it.
        let d = Delta::Splice {
            at: 1,
            remove: 0,
            insert: "x".into(),
        };
        assert_eq!(
            d.apply("é").unwrap_err(),
            ApplyError::NotCharBoundary { offset: 1 }
        );
    }

    #[test]
    fn apply_error_displays() {
        let e = ApplyError::OutOfRange {
            at: 1,
            remove: 2,
            len: 0,
        };
        assert!(e.to_string().contains("out of range"));
    }

    #[test]
    fn client_open_round_trips_through_json() {
        let msg = ClientMsg::Open { since_seq: 7 };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"open","since_seq":7}"#);
        assert_eq!(serde_json::from_str::<ClientMsg>(&json).unwrap(), msg);
    }

    #[test]
    fn client_edit_round_trips_through_json() {
        let msg = ClientMsg::Edit {
            base_seq: 3,
            delta: Delta::Splice {
                at: 0,
                remove: 0,
                insert: "x".into(),
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(serde_json::from_str::<ClientMsg>(&json).unwrap(), msg);
        // The delta is tagged independently of the message.
        assert!(json.contains(r#""type":"edit""#));
        assert!(json.contains(r#""kind":"splice""#));
    }

    #[test]
    fn server_messages_round_trip_through_json() {
        for msg in [
            ServerMsg::Ack { seq: 9 },
            ServerMsg::Sync {
                seq: 2,
                text: "body".into(),
            },
            ServerMsg::BackedUp {
                commit_sha: Some("deadbeef".into()),
            },
            ServerMsg::BackedUp { commit_sha: None },
        ] {
            let json = serde_json::to_string(&msg).unwrap();
            assert_eq!(serde_json::from_str::<ServerMsg>(&json).unwrap(), msg);
        }
    }

    #[test]
    fn backed_up_omitted_sha_serializes_as_null() {
        let json = serde_json::to_string(&ServerMsg::BackedUp { commit_sha: None }).unwrap();
        assert_eq!(json, r#"{"type":"backed_up","commit_sha":null}"#);
    }
}
