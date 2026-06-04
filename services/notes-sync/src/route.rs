//! Pure, native-testable request-routing helpers shared by the Worker
//! `fetch` entrypoint and the Durable Object. Kept off the wasm-only path
//! so `cargo test` exercises them on the host (the `worker` runtime types
//! only exist on `wasm32`).

/// Extract the note id from a websocket path of the form `/note/<id>/ws`.
///
/// Returns `None` for any other shape, an empty id, or an id containing a
/// path separator — so a caller can treat `Some(id)` as a validated,
/// single-segment note identifier.
pub fn parse_ws_note_id(path: &str) -> Option<&str> {
    let id = path.strip_prefix("/note/")?.strip_suffix("/ws")?;
    if id.is_empty() || id.contains('/') {
        return None;
    }
    Some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_well_formed_ws_path() {
        assert_eq!(parse_ws_note_id("/note/abc123/ws"), Some("abc123"));
    }

    #[test]
    fn rejects_missing_prefix_or_suffix() {
        assert_eq!(parse_ws_note_id("/abc123/ws"), None);
        assert_eq!(parse_ws_note_id("/note/abc123"), None);
        assert_eq!(parse_ws_note_id("/"), None);
    }

    #[test]
    fn rejects_empty_id() {
        assert_eq!(parse_ws_note_id("/note//ws"), None);
    }

    #[test]
    fn rejects_multi_segment_id() {
        assert_eq!(parse_ws_note_id("/note/a/b/ws"), None);
    }
}
