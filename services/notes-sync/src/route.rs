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

/// Why a rename request can't proceed. Returned by [`plan_rename`] so the
/// wasm handler can map each case to an HTTP status without re-deriving the
/// rules.
#[derive(Debug, PartialEq, Eq)]
pub enum RenameError {
    /// Either the source or destination id is not a valid note path.
    InvalidId,
    /// Source and destination are the same note — nothing to move.
    SameId,
    /// No committed note exists at the source path. `/rename` is the trust
    /// boundary (the front end only offers rename on listed notes, but a
    /// raw request need not), and a rename of a missing source would
    /// otherwise create a spurious empty note at the destination.
    SourceMissing,
}

/// How a rename of `old` to `new` should be carried out, decided from the
/// committed vault listing. Returned by [`plan_rename`].
#[derive(Debug, PartialEq, Eq)]
pub enum RenamePlan {
    /// Source present, destination free — a normal create-before-delete
    /// move.
    Fresh,
    /// Source present *and* a file already sits at the destination. A prior
    /// attempt created the destination but failed before removing the
    /// source (the orchestration is not atomic), so the caller re-drives
    /// the remaining steps idempotently — but only after confirming the
    /// destination body is the one being carried; a destination holding a
    /// *different* body is a genuine collision the caller rejects as a
    /// conflict.
    Resume,
}

/// Plan a `rename`/move of `old` to `new` against the current vault. Both
/// ids must be valid note paths (the same rule the websocket route and
/// `/delete` enforce), the move must actually change the path, and the
/// source must exist. `existing` is the committed vault listing (see
/// `list_note_paths`).
///
/// A destination that already exists is *not* a hard error: combined with a
/// still-present source it signals a half-finished earlier attempt that the
/// caller can resume (see [`RenamePlan::Resume`]). The caller disambiguates
/// a true collision from a resumable in-progress rename by comparing the
/// destination's committed body against the body being carried.
pub fn plan_rename(old: &str, new: &str, existing: &[String]) -> Result<RenamePlan, RenameError> {
    if !is_valid_note_id(old) || !is_valid_note_id(new) {
        return Err(RenameError::InvalidId);
    }
    if old == new {
        return Err(RenameError::SameId);
    }
    if !existing.iter().any(|n| n == old) {
        return Err(RenameError::SourceMissing);
    }
    if existing.iter().any(|n| n == new) {
        return Ok(RenamePlan::Resume);
    }
    Ok(RenamePlan::Fresh)
}

/// Pick the body to carry in a rename. The source Durable Object's
/// materialized text is authoritative when present, but a cold or
/// never-opened DO returns an empty body even though git holds the
/// committed note (the DO does not hydrate from git on open). Falling back
/// to the committed git blob keeps the move from carrying `""` and erasing
/// the source when the body is dropped — turning a transient display gap
/// into permanent data loss.
pub fn choose_rename_body(do_text: &str, git_text: Option<&str>) -> String {
    if do_text.is_empty() {
        git_text.unwrap_or_default().to_owned()
    } else {
        do_text.to_owned()
    }
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

    #[test]
    fn plan_rename_accepts_a_move_to_a_free_path() {
        let existing = vec!["scratch".to_string(), "projects/foo".to_string()];
        assert_eq!(
            plan_rename("scratch", "projects/bar", &existing),
            Ok(RenamePlan::Fresh)
        );
    }

    #[test]
    fn plan_rename_rejects_an_invalid_id_on_either_side() {
        let existing = vec!["ok".to_string()];
        assert_eq!(
            plan_rename("../escape", "ok2", &existing),
            Err(RenameError::InvalidId)
        );
        assert_eq!(
            plan_rename("ok", "has space", &existing),
            Err(RenameError::InvalidId)
        );
        assert_eq!(
            plan_rename("", "ok2", &existing),
            Err(RenameError::InvalidId)
        );
    }

    #[test]
    fn plan_rename_rejects_a_no_op_move() {
        let existing = vec!["scratch".to_string()];
        assert_eq!(
            plan_rename("scratch", "scratch", &existing),
            Err(RenameError::SameId)
        );
    }

    #[test]
    fn plan_rename_rejects_a_missing_source() {
        // A valid-id rename of a note that isn't in the vault must not
        // silently create an empty note at the destination.
        let existing = vec!["scratch".to_string()];
        assert_eq!(
            plan_rename("ghost", "projects/bar", &existing),
            Err(RenameError::SourceMissing)
        );
    }

    #[test]
    fn plan_rename_resumes_when_both_paths_are_present() {
        // Source still present *and* destination already committed → a prior
        // attempt got past the create but not the source delete. The caller
        // resumes (and disambiguates a true collision by body).
        let existing = vec!["scratch".to_string(), "projects/foo".to_string()];
        assert_eq!(
            plan_rename("scratch", "projects/foo", &existing),
            Ok(RenamePlan::Resume)
        );
    }

    #[test]
    fn choose_rename_body_prefers_a_non_empty_do_body() {
        assert_eq!(choose_rename_body("live", Some("stale git")), "live");
        assert_eq!(choose_rename_body("live", None), "live");
    }

    #[test]
    fn choose_rename_body_falls_back_to_git_for_a_cold_do() {
        // A cold/never-opened DO exports "" though git holds the note —
        // carry the committed blob rather than erasing it on the move.
        assert_eq!(
            choose_rename_body("", Some("committed body")),
            "committed body"
        );
        assert_eq!(choose_rename_body("", None), "");
    }
}
