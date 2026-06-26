//! Pure, native-testable request-routing helpers shared by the Worker
//! `fetch` entrypoint and the Durable Object. Kept off the wasm-only path
//! so `cargo test` exercises them on the host (the `worker` runtime types
//! only exist on `wasm32`).

/// Extract the note id from a websocket path of the form `/note/<id>/ws`.
///
/// The id may be a `/`-separated path (`projects/foo/idea`) so a vault
/// nests under `notes/<id>.md`. The returned value is validated to be safe
/// as both a Durable Object key and a git path: it has no empty segments
/// (so no leading, trailing, or doubled slashes), no `.`/`..` segment (so
/// no traversal out of `notes/`), and every segment is made only of
/// `[A-Za-z0-9._-]`. A caller can treat `Some(id)` as a trusted note path.
///
/// Returns `None` for any other shape or a segment that fails those rules.
pub fn parse_ws_note_id(path: &str) -> Option<&str> {
    let id = path.strip_prefix("/note/")?.strip_suffix("/ws")?;
    is_valid_note_id(id).then_some(id)
}

/// Whether `id` is a valid note path: non-empty, and every `/`-separated
/// segment is safe as a path component (see `is_safe_segment`). The same
/// rule gates the websocket route and the mutation endpoints, so a note id
/// that reaches a Durable Object or a git path is always trusted.
pub fn is_valid_note_id(id: &str) -> bool {
    !id.is_empty() && id.split('/').all(is_safe_segment)
}

/// A single path segment is safe when it is non-empty, isn't a `.`/`..`
/// traversal token, and contains only unreserved path characters.
fn is_safe_segment(segment: &str) -> bool {
    if segment.is_empty() || segment == "." || segment == ".." {
        return false;
    }
    segment
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_well_formed_ws_path() {
        assert_eq!(parse_ws_note_id("/note/abc123/ws"), Some("abc123"));
    }

    #[test]
    fn accepts_a_nested_path_id() {
        assert_eq!(
            parse_ws_note_id("/note/projects/foo/idea/ws"),
            Some("projects/foo/idea")
        );
    }

    #[test]
    fn accepts_unreserved_segment_characters() {
        assert_eq!(
            parse_ws_note_id("/note/daily/2026-06-25_draft.v2/ws"),
            Some("daily/2026-06-25_draft.v2")
        );
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
    fn rejects_empty_segments_from_stray_slashes() {
        // Leading, trailing, and doubled slashes all yield an empty segment.
        assert_eq!(parse_ws_note_id("/note//foo/ws"), None);
        assert_eq!(parse_ws_note_id("/note/foo//ws"), None);
        assert_eq!(parse_ws_note_id("/note/foo//bar/ws"), None);
    }

    #[test]
    fn rejects_dot_traversal_segments() {
        assert_eq!(parse_ws_note_id("/note/../secrets/ws"), None);
        assert_eq!(parse_ws_note_id("/note/foo/../../etc/ws"), None);
        assert_eq!(parse_ws_note_id("/note/./foo/ws"), None);
    }

    #[test]
    fn rejects_unsafe_segment_characters() {
        assert_eq!(parse_ws_note_id("/note/foo bar/ws"), None);
        assert_eq!(parse_ws_note_id("/note/foo:bar/ws"), None);
        assert_eq!(parse_ws_note_id("/note/foo%2Fbar/ws"), None);
    }

    #[test]
    fn is_valid_note_id_accepts_safe_paths_and_rejects_the_rest() {
        assert!(is_valid_note_id("scratch"));
        assert!(is_valid_note_id("projects/foo/idea"));
        assert!(!is_valid_note_id(""));
        assert!(!is_valid_note_id("/leading"));
        assert!(!is_valid_note_id("trailing/"));
        assert!(!is_valid_note_id("a//b"));
        assert!(!is_valid_note_id("../escape"));
        assert!(!is_valid_note_id("has space"));
    }
}
