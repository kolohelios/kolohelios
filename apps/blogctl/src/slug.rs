use crate::error::{Error, Result};

/// Slugify a title into a kebab-case identifier suitable for a filename.
///
/// Lowercases ASCII letters, keeps digits, collapses any run of other
/// characters into a single `-`, and trims leading/trailing `-`. Non-ASCII
/// characters are replaced like punctuation — a small, predictable rule for
/// English-language posts; we'll grow it (transliteration, unicode
/// normalization) when a real post needs it.
pub fn slugify(title: &str) -> Result<String> {
    let mut out = String::with_capacity(title.len());
    let mut prev_dash = true;
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            for lower in c.to_lowercase() {
                out.push(lower);
            }
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }

    if out.is_empty() {
        return Err(Error::EmptyTitle {
            value: title.to_string(),
        });
    }
    Ok(out)
}

/// Validate a slug — must match `^[a-z0-9][a-z0-9-]*$` with no `--` runs and
/// no trailing `-`. Returns the slug as-is if valid.
pub fn validate(slug: &str) -> Result<&str> {
    if slug.is_empty() {
        return Err(Error::InvalidSlug {
            value: slug.to_string(),
            reason: "empty".into(),
        });
    }
    let bytes = slug.as_bytes();
    let first = bytes[0];
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return Err(Error::InvalidSlug {
            value: slug.to_string(),
            reason: "must start with [a-z0-9]".into(),
        });
    }
    if bytes[bytes.len() - 1] == b'-' {
        return Err(Error::InvalidSlug {
            value: slug.to_string(),
            reason: "cannot end with '-'".into(),
        });
    }
    let mut prev_dash = false;
    for &b in bytes {
        let ok = b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-';
        if !ok {
            return Err(Error::InvalidSlug {
                value: slug.to_string(),
                reason: format!("contains invalid byte {:?}", b as char),
            });
        }
        if b == b'-' && prev_dash {
            return Err(Error::InvalidSlug {
                value: slug.to_string(),
                reason: "contains '--'".into(),
            });
        }
        prev_dash = b == b'-';
    }
    Ok(slug)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_lowercases_ascii_words() {
        assert_eq!(slugify("Hello World").unwrap(), "hello-world");
    }

    #[test]
    fn slugify_collapses_punctuation_and_spaces() {
        assert_eq!(
            slugify("It's a Beautiful... Day!").unwrap(),
            "it-s-a-beautiful-day"
        );
    }

    #[test]
    fn slugify_strips_leading_and_trailing_separators() {
        assert_eq!(slugify("  --hello--  ").unwrap(), "hello");
    }

    #[test]
    fn slugify_keeps_digits() {
        assert_eq!(slugify("rust 2026 retro").unwrap(), "rust-2026-retro");
    }

    #[test]
    fn slugify_rejects_empty_titles() {
        assert!(matches!(slugify("").unwrap_err(), Error::EmptyTitle { .. }));
        assert!(matches!(
            slugify("   ").unwrap_err(),
            Error::EmptyTitle { .. }
        ));
        assert!(matches!(
            slugify("???").unwrap_err(),
            Error::EmptyTitle { .. }
        ));
    }

    #[test]
    fn slugify_treats_non_ascii_as_separators() {
        // Predictable, lossy fallback. Good-enough until a real post forces
        // transliteration. Asserted as properties rather than a literal so
        // the test isn't sensitive to the exact post-strip token sequence.
        let out = slugify("café résumé").unwrap();
        assert!(!out.is_empty());
        assert!(!out.contains('é'));
        assert!(
            out.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "got: {out}"
        );
        assert!(!out.starts_with('-') && !out.ends_with('-'));
        assert!(!out.contains("--"));
    }

    #[test]
    fn validate_accepts_well_formed_slugs() {
        for s in ["a", "ab", "a1", "hello-world", "rust-2026-retro", "x9"] {
            assert!(validate(s).is_ok(), "{s}");
        }
    }

    #[test]
    fn validate_rejects_malformed_slugs() {
        for s in ["", "-foo", "foo-", "foo--bar", "Foo", "foo_bar", "foo bar"] {
            assert!(matches!(validate(s), Err(Error::InvalidSlug { .. })), "{s}");
        }
    }
}
