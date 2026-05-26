//! Shared conventional-commit title validator. Used by `shaka commit
//! lint` to gate commits and by `shaka issue create` to gate issue
//! titles (so an issue's title can be used as a commit title later
//! without re-editing).
//!
//! The grammar accepted is `<type>(<scope>): <subject>` or
//! `<type>: <subject>`, with `<type>` drawn from `TYPES` and `<scope>`
//! a non-empty string of `[A-Za-z0-9_/-]`. Errors are structured so
//! both call sites can surface a specific reason and the valid set.
//!
//! The leading parsing is colon-anchored rather than regex-driven so
//! that the error path can pinpoint *which* component is bad. The
//! length cap (`TITLE_MAX`) lives in `commit.rs` since it's policy,
//! not grammar.

/// Conventional commit types accepted across the repo. Anything outside
/// this set fails validation; expanding the set is intentionally a
/// code-change rather than config so all consumers stay in lockstep.
pub const TYPES: &[&str] = &[
    "feat", "fix", "docs", "chore", "refactor", "test", "style", "perf", "ci", "build",
];

/// Why a conventional-commit title failed validation. Variants are
/// exhaustive of the structural failure modes the validator can
/// produce; callers `match` on this to render the error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TitleError {
    /// No `": "` separator was found between the prefix and the subject.
    MissingSeparator,
    /// The subject (the text after `": "`) was empty.
    EmptySubject,
    /// A parenthesized scope was present but malformed (unclosed parens,
    /// empty scope, or disallowed characters).
    InvalidScope { scope: String },
    /// The type prefix (before `(` or `:`) was not in `TYPES`.
    UnknownType { found: String },
}

impl std::fmt::Display for TitleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSeparator => write!(
                f,
                "title does not match `<type>(<scope>): <subject>`: missing `: ` separator"
            ),
            Self::EmptySubject => write!(
                f,
                "title does not match `<type>(<scope>): <subject>`: subject is empty"
            ),
            Self::InvalidScope { scope } => write!(
                f,
                "scope `{scope}` is invalid (must be a non-empty `[A-Za-z0-9_/-]+`)"
            ),
            Self::UnknownType { found } => {
                write!(f, "type `{found}` is not one of {{{}}}", TYPES.join(", "))
            }
        }
    }
}

/// Validate `title` against the conventional-commit shape. Returns
/// `Ok(())` when the title is acceptable; otherwise the first failure
/// encountered (in parse order, so the message points at the first
/// thing that didn't match).
pub fn validate(title: &str) -> Result<(), TitleError> {
    let Some(colon_idx) = title.find(": ") else {
        return Err(TitleError::MissingSeparator);
    };
    let prefix = &title[..colon_idx];
    let subject = &title[colon_idx + 2..];

    if subject.is_empty() {
        return Err(TitleError::EmptySubject);
    }

    let (type_part, scope_part) = match prefix.find('(') {
        Some(open) => {
            if !prefix.ends_with(')') {
                return Err(TitleError::InvalidScope {
                    scope: prefix[open..].to_string(),
                });
            }
            let close = prefix.len() - 1;
            (&prefix[..open], Some(&prefix[open + 1..close]))
        }
        None => (prefix, None),
    };

    if !TYPES.contains(&type_part) {
        return Err(TitleError::UnknownType {
            found: type_part.to_string(),
        });
    }

    if let Some(scope) = scope_part {
        if scope.is_empty() {
            return Err(TitleError::InvalidScope {
                scope: String::new(),
            });
        }
        if !scope
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '/')
        {
            return Err(TitleError::InvalidScope {
                scope: scope.to_string(),
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_canonical_shapes() {
        assert!(validate("feat: add thing").is_ok());
        assert!(validate("feat(shaka): add thing").is_ok());
        assert!(validate("ci(infra/devbox): tweak").is_ok());
        assert!(validate("chore(deps): bump x").is_ok());
    }

    #[test]
    fn rejects_missing_separator() {
        assert_eq!(
            validate("feat add thing"),
            Err(TitleError::MissingSeparator)
        );
    }

    #[test]
    fn rejects_empty_subject() {
        assert_eq!(validate("feat: "), Err(TitleError::EmptySubject));
    }

    #[test]
    fn rejects_unknown_type() {
        assert_eq!(
            validate("frob: x"),
            Err(TitleError::UnknownType {
                found: "frob".into()
            })
        );
    }

    #[test]
    fn rejects_empty_scope() {
        assert_eq!(
            validate("feat(): x"),
            Err(TitleError::InvalidScope {
                scope: String::new()
            })
        );
    }

    #[test]
    fn rejects_unclosed_scope() {
        assert!(matches!(
            validate("feat(shaka: x"),
            Err(TitleError::InvalidScope { .. })
        ));
    }

    #[test]
    fn rejects_scope_with_disallowed_chars() {
        assert!(matches!(
            validate("feat(scope with spaces): x"),
            Err(TitleError::InvalidScope { .. })
        ));
    }

    #[test]
    fn display_lists_valid_types() {
        let err = validate("frob: x").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("feat"));
        assert!(msg.contains("chore"));
    }
}
