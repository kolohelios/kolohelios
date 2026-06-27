//! Pure editor client logic: the document state and the protocol
//! decisions, native-tested off the browser DOM/socket glue in `dom`.
//! Shares the `notes-protocol` types with the Durable Object so the two
//! ends can't drift.
//!
//! The state machine keeps **at most one edit in flight** and coalesces
//! keystrokes that arrive while it's outstanding into a single `pending`
//! body. That serialization is what makes the whole-body v1 protocol safe
//! under fast typing: every `Edit` we send carries the current `seq` as
//! its `base_seq`, so it can never be self-stale, so the server never
//! answers with a `Sync` that would overwrite what the user is typing.

use notes_protocol::{frontmatter, ClientMsg, Delta, Seq, ServerMsg};

/// The note id (the `/`-separated path) carried in a `…/note/<id>/ws`
/// WebSocket URL, used as the editor's title fallback. Returns `None` if
/// the URL isn't that shape. Note ids are restricted to unreserved
/// characters per segment, so the percent-encoded path in the URL equals
/// the human-readable id.
pub fn note_id_from_ws_url(ws_url: &str) -> Option<String> {
    let after = ws_url.split_once("/note/")?.1;
    let path = after.strip_suffix("/ws")?;
    (!path.is_empty()).then(|| path.to_owned())
}

/// The display title for a note: its front-matter title when present and
/// non-blank, otherwise `fallback` (the note id). Malformed front-matter
/// falls back too — the surface should never go titleless on a parse slip.
pub fn display_title(body: &str, fallback: &str) -> String {
    frontmatter::parse(body)
        .ok()
        .and_then(|(fm, _body)| fm.title)
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| fallback.to_owned())
}

/// The editor's view of the document.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ClientState {
    /// Last sequence the server confirmed.
    pub seq: Seq,
    /// Body the server has confirmed at `seq` (or is about to, once the
    /// in-flight edit acks). Not the live textarea — the browser owns that.
    pub text: String,
    /// Body of the edit awaiting an `Ack` (`None` when idle). At most one
    /// is ever outstanding, so its `base_seq` is always the current `seq`.
    sending: Option<String>,
    /// Newest local body queued behind the in-flight edit, coalesced to
    /// just the latest — intermediate keystrokes are dropped, not queued.
    pending: Option<String>,
}

/// What the UI should do to the editing surface after folding a server
/// frame. Only `Replace` touches the textarea.
#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    /// Adopt `text` on the surface (an idle `Sync`: initial load/reconnect).
    Replace(String),
    /// The note reached git — `commit_sha` when known. For a future
    /// "backed up" status line; the surface is untouched.
    BackedUp(Option<String>),
    /// Nothing to redraw: an `Ack`, or a `Sync` we declined to adopt
    /// because the user has unsaved local edits.
    None,
}

impl ClientState {
    /// The message to send on (re)connect: resync from the last sequence
    /// seen, so a reconnecting editor catches up rather than clobbers.
    pub fn open(&self) -> ClientMsg {
        ClientMsg::Open {
            since_seq: self.seq,
        }
    }

    /// Fold a local keystroke (the full textarea body). Returns the `Edit`
    /// to send now, or `None` if an edit is already in flight and this was
    /// coalesced into `pending`. The next pending body is flushed on `Ack`.
    pub fn local_edit(&mut self, body: &str) -> Option<ClientMsg> {
        if self.sending.is_some() {
            self.pending = Some(body.to_owned());
            None
        } else {
            Some(self.start_send(body.to_owned()))
        }
    }

    /// Fold a server frame into the state. Returns the surface effect and
    /// an optional follow-up `Edit` (flushing a coalesced keystroke now
    /// that the in-flight slot is free).
    pub fn apply(&mut self, msg: ServerMsg) -> (Effect, Option<ClientMsg>) {
        match msg {
            ServerMsg::Ack { seq } => {
                self.seq = seq;
                if let Some(sent) = self.sending.take() {
                    self.text = sent;
                }
                (Effect::None, self.flush_pending())
            }
            ServerMsg::Sync { seq, text } => {
                self.seq = seq;
                if self.sending.is_some() || self.pending.is_some() {
                    // The user has local edits outstanding, so the server's
                    // body is behind us. Keep what they typed on screen and
                    // re-send it on the new base rather than overwriting.
                    self.text = text;
                    let local = self.pending.take().or_else(|| self.sending.take());
                    self.sending = None;
                    (Effect::None, local.map(|b| self.start_send(b)))
                } else {
                    // Idle: adopt the server body (initial load / reconnect).
                    self.text = text.clone();
                    (Effect::Replace(text), None)
                }
            }
            ServerMsg::BackedUp { commit_sha } => (Effect::BackedUp(commit_sha), None),
        }
    }

    /// Send the next coalesced body if it differs from what the server now
    /// holds; otherwise the slot stays idle.
    fn flush_pending(&mut self) -> Option<ClientMsg> {
        let next = self.pending.take()?;
        if next == self.text {
            return None;
        }
        Some(self.start_send(next))
    }

    /// Build the `Edit` for `body` on top of the current `seq` and mark it
    /// in flight (the single outstanding slot).
    fn start_send(&mut self, body: String) -> ClientMsg {
        let msg = ClientMsg::Edit {
            base_seq: self.seq,
            delta: Delta::Whole { text: body.clone() },
        };
        self.sending = Some(body);
        msg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(base_seq: Seq, text: &str) -> ClientMsg {
        ClientMsg::Edit {
            base_seq,
            delta: Delta::Whole { text: text.into() },
        }
    }

    #[test]
    fn open_resyncs_from_the_last_seen_sequence() {
        let state = ClientState {
            seq: 7,
            text: "body".into(),
            ..Default::default()
        };
        assert_eq!(state.open(), ClientMsg::Open { since_seq: 7 });
    }

    #[test]
    fn an_idle_edit_is_sent_immediately_on_the_current_sequence() {
        let mut s = ClientState {
            seq: 3,
            text: "old".into(),
            ..Default::default()
        };
        assert_eq!(s.local_edit("new body"), Some(edit(3, "new body")));
    }

    #[test]
    fn keystrokes_behind_an_in_flight_edit_are_coalesced() {
        let mut s = ClientState::default();
        // First keystroke goes out immediately.
        assert_eq!(s.local_edit("a"), Some(edit(0, "a")));
        // Everything typed before the Ack is coalesced to just the latest.
        assert_eq!(s.local_edit("ab"), None);
        assert_eq!(s.local_edit("abc"), None);
        assert_eq!(s.local_edit("abcd"), None);
    }

    #[test]
    fn fast_typing_never_goes_stale_or_clobbers() {
        // The bug this fixes: pipelined edits with the same base_seq → the
        // server marks the later ones stale → sends a Sync that overwrites
        // the textarea. With serialization the follow-up carries the
        // advanced base_seq, so it's never stale and no Sync is provoked.
        let mut s = ClientState::default();
        assert_eq!(
            s.apply(ServerMsg::Sync {
                seq: 0,
                text: String::new(),
            }),
            (Effect::Replace(String::new()), None)
        );

        assert_eq!(s.local_edit("a"), Some(edit(0, "a"))); // sent
        assert_eq!(s.local_edit("ab"), None); // coalesced
        assert_eq!(s.local_edit("abc"), None); // coalesced to latest

        // Ack for "a": seq → 1, and the coalesced "abc" flushes on base 1.
        let (effect, follow) = s.apply(ServerMsg::Ack { seq: 1 });
        assert_eq!(effect, Effect::None);
        assert_eq!(follow, Some(edit(1, "abc"))); // base 1, never stale

        // Ack for "abc": nothing pending, nothing to send.
        let (effect, follow) = s.apply(ServerMsg::Ack { seq: 2 });
        assert_eq!(effect, Effect::None);
        assert_eq!(follow, None);
        assert_eq!(s.seq, 2);
        assert_eq!(s.text, "abc");
    }

    #[test]
    fn ack_with_no_change_queued_sends_nothing() {
        let mut s = ClientState::default();
        s.local_edit("a"); // in flight
        let (effect, follow) = s.apply(ServerMsg::Ack { seq: 1 });
        assert_eq!(effect, Effect::None);
        assert_eq!(follow, None);
        assert_eq!(s.text, "a");
    }

    #[test]
    fn an_idle_sync_adopts_the_server_body() {
        let mut s = ClientState::default();
        let (effect, follow) = s.apply(ServerMsg::Sync {
            seq: 5,
            text: "server body".into(),
        });
        assert_eq!(effect, Effect::Replace("server body".into()));
        assert_eq!(follow, None);
        assert_eq!(s.seq, 5);
        assert_eq!(s.text, "server body");
    }

    #[test]
    fn a_sync_with_unsaved_local_edits_keeps_local_and_resends() {
        // A reconnect Sync must not overwrite text the user typed but the
        // server hasn't confirmed; keep it on screen and re-send on the new
        // base sequence.
        let mut s = ClientState::default();
        s.local_edit("typed locally"); // now in flight
        let (effect, follow) = s.apply(ServerMsg::Sync {
            seq: 9,
            text: "stale server body".into(),
        });
        assert_eq!(effect, Effect::None); // surface untouched
        assert_eq!(follow, Some(edit(9, "typed locally"))); // re-sent on base 9
        assert_eq!(s.seq, 9);
    }

    #[test]
    fn backed_up_surfaces_the_commit_sha_without_redraw() {
        let mut s = ClientState::default();
        assert_eq!(
            s.apply(ServerMsg::BackedUp {
                commit_sha: Some("abc123".into()),
            }),
            (Effect::BackedUp(Some("abc123".into())), None)
        );
    }

    #[test]
    fn note_id_is_extracted_from_the_ws_url() {
        assert_eq!(
            note_id_from_ws_url("wss://notes.example.com/note/projects/idea/ws"),
            Some("projects/idea".to_owned())
        );
        assert_eq!(
            note_id_from_ws_url("ws://localhost:8787/note/scratch/ws"),
            Some("scratch".to_owned())
        );
    }

    #[test]
    fn note_id_is_none_for_a_non_note_url() {
        assert_eq!(note_id_from_ws_url("wss://example.com/me"), None);
        assert_eq!(note_id_from_ws_url("wss://example.com/note//ws"), None);
    }

    #[test]
    fn display_title_prefers_the_frontmatter_title() {
        let body = "---\ntitle: My Real Title\n---\nbody\n";
        assert_eq!(display_title(body, "projects/idea"), "My Real Title");
    }

    #[test]
    fn display_title_falls_back_to_the_id_without_frontmatter() {
        assert_eq!(
            display_title("just a body\n", "projects/idea"),
            "projects/idea"
        );
    }

    #[test]
    fn display_title_falls_back_when_the_title_is_blank() {
        let body = "---\ntitle: \"   \"\n---\nbody\n";
        assert_eq!(display_title(body, "scratch"), "scratch");
    }

    #[test]
    fn display_title_falls_back_on_malformed_frontmatter() {
        let body = "---\ntags: [unterminated\n---\nbody\n";
        assert_eq!(display_title(body, "scratch"), "scratch");
    }
}
