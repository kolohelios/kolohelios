//! Pure editor client logic: the document state and the protocol
//! decisions, native-tested off the browser DOM/socket glue in `dom`.
//! Shares the `notes-protocol` types with the Durable Object so the two
//! ends can't drift.

use notes_protocol::{ClientMsg, Delta, Seq, ServerMsg};

/// The editor's view of the document: the last sequence confirmed by the
/// server and the body at that sequence.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ClientState {
    pub seq: Seq,
    pub text: String,
}

/// What the UI should do after folding a server message in.
#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    /// Adopt `text` on the editing surface (a full `Sync`).
    Replace(String),
    /// An edit was accepted — nothing to redraw.
    Acked,
    /// The note was committed to git (`commit_sha` when known).
    BackedUp(Option<String>),
}

impl ClientState {
    /// The message to send on (re)connect: resync from the last sequence
    /// seen, so a reconnecting editor catches up rather than clobbers.
    pub fn open(&self) -> ClientMsg {
        ClientMsg::Open {
            since_seq: self.seq,
        }
    }

    /// The edit message for a new local body. v1 sends the whole body (the
    /// always-correct fallback delta); a splice path can replace this
    /// later without touching the protocol or this signature.
    pub fn edit(&self, new_text: &str) -> ClientMsg {
        ClientMsg::Edit {
            base_seq: self.seq,
            delta: Delta::Whole {
                text: new_text.to_owned(),
            },
        }
    }

    /// Fold a server message into the state, returning the UI effect. A
    /// `Sync` resets the confirmed sequence and body; an `Ack` only
    /// advances the sequence (the local body already reflects the edit
    /// the server just accepted).
    pub fn apply(&mut self, msg: ServerMsg) -> Effect {
        match msg {
            ServerMsg::Sync { seq, text } => {
                self.seq = seq;
                self.text = text.clone();
                Effect::Replace(text)
            }
            ServerMsg::Ack { seq } => {
                self.seq = seq;
                Effect::Acked
            }
            ServerMsg::BackedUp { commit_sha } => Effect::BackedUp(commit_sha),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_resyncs_from_the_last_seen_sequence() {
        let state = ClientState {
            seq: 7,
            text: "body".into(),
        };
        assert_eq!(state.open(), ClientMsg::Open { since_seq: 7 });
    }

    #[test]
    fn edit_sends_the_whole_body_on_top_of_the_current_sequence() {
        let state = ClientState {
            seq: 3,
            text: "old".into(),
        };
        assert_eq!(
            state.edit("new body"),
            ClientMsg::Edit {
                base_seq: 3,
                delta: Delta::Whole {
                    text: "new body".into()
                },
            }
        );
    }

    #[test]
    fn sync_replaces_the_surface_and_resets_state() {
        let mut state = ClientState::default();
        let effect = state.apply(ServerMsg::Sync {
            seq: 5,
            text: "server body".into(),
        });
        assert_eq!(effect, Effect::Replace("server body".into()));
        assert_eq!(state.seq, 5);
        assert_eq!(state.text, "server body");
    }

    #[test]
    fn ack_only_advances_the_sequence() {
        let mut state = ClientState {
            seq: 4,
            text: "local".into(),
        };
        assert_eq!(state.apply(ServerMsg::Ack { seq: 5 }), Effect::Acked);
        assert_eq!(state.seq, 5);
        // The body is untouched — it already holds what we sent.
        assert_eq!(state.text, "local");
    }

    #[test]
    fn backed_up_surfaces_the_commit_sha() {
        let mut state = ClientState::default();
        assert_eq!(
            state.apply(ServerMsg::BackedUp {
                commit_sha: Some("abc123".into())
            }),
            Effect::BackedUp(Some("abc123".into()))
        );
    }

    #[test]
    fn an_edit_after_an_ack_uses_the_acked_sequence_as_its_base() {
        let mut state = ClientState::default();
        state.apply(ServerMsg::Sync {
            seq: 1,
            text: "a".into(),
        });
        state.apply(ServerMsg::Ack { seq: 2 });
        match state.edit("ab") {
            ClientMsg::Edit { base_seq, .. } => assert_eq!(base_seq, 2),
            other => panic!("expected Edit, got {other:?}"),
        }
    }
}
