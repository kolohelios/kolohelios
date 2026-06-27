//! Human-authored note metadata, carried as a YAML front-matter block at
//! the head of the note body.
//!
//! The note body is the one source of truth the Durable Object stores and
//! replays — it treats the body as opaque text. Front-matter rides *inside*
//! that text (a `---`-delimited YAML block at the top), so the sync/replay
//! invariant is untouched: the DO never parses it, edits to it flow through
//! the same `Delta` path as any other keystroke. Everything else the AI
//! features derive (title, tags, links, embeddings, graph edges) is keyed by
//! the [`content_hash`] of the body so staleness is detectable.
//!
//! The shape mirrors Obsidian's front-matter so a vault stays portable:
//!
//! ```text
//! ---
//! title: My Note
//! tags: [rust, notes]
//! aliases: [scratchpad]
//! ---
//! the note body…
//! ```
//!
//! A note with no leading front-matter block is perfectly valid — [`parse`]
//! returns [`FrontMatter::default`] and the whole string as the body.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use snafu::{ResultExt, Snafu};

/// Line that opens and closes the front-matter block.
const DELIM: &str = "---";

/// Human-authored metadata for a note. Every field is optional and
/// defaults to empty, so a fresh note (no front-matter) deserializes
/// cleanly and a partial block (only `title`, say) is fine. Unknown keys
/// are captured in [`extra`](Self::extra) rather than rejected *or*
/// dropped, so an Obsidian vault carrying extra front-matter keys
/// (`created`, `cssclasses`, `publish`, …) round-trips losslessly —
/// metadata as well as body — through [`parse`] and [`set_title`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FrontMatter {
    /// Display title. When absent the editor falls back to the note id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Free-form tags.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Alternate names the note is also known by (Obsidian aliases).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Any other front-matter keys the note carries. These are not part of
    /// the canonical shape this crate reasons about, but a vault stays
    /// portable only if they survive a write-back, so they are preserved
    /// verbatim and re-emitted after the known fields on render. The CUE
    /// schema is open (`...`) to match.
    #[serde(flatten)]
    pub extra: serde_yaml_ng::Mapping,
}

/// Why front-matter could not be parsed or rendered.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum Error {
    /// The text between the `---` delimiters was present but was not valid
    /// YAML (or did not match the front-matter shape).
    #[snafu(display("malformed YAML front-matter: {source}"))]
    Yaml { source: serde_yaml_ng::Error },
    /// Serializing the front-matter back to YAML failed.
    #[snafu(display("could not serialize front-matter: {source}"))]
    Serialize { source: serde_yaml_ng::Error },
}

/// Split a note into its raw front-matter YAML and body, purely
/// structurally — by the `---` delimiter lines, *not* by whether the YAML
/// parses. Returns `None` when the note does not open with a `---` line
/// followed by a closing `---` line (i.e. there is no front-matter block).
///
/// The opening delimiter must be the very first line; the closing
/// delimiter is the next line that is exactly `---` (a body line that
/// merely starts with `---`, like `--- not a delimiter ---`, is not
/// mistaken for it). An empty block (`---\n---\n`) is recognized too.
fn split(note: &str) -> Option<(&str, &str)> {
    let after_open = note
        .strip_prefix("---\n")
        .or_else(|| note.strip_prefix("---\r\n"))?;

    let mut offset = 0;
    loop {
        let line_end = after_open[offset..]
            .find('\n')
            .map_or(after_open.len(), |i| offset + i);
        let line = &after_open[offset..line_end];
        // Tolerate a `\r` left on the line by CRLF endings.
        if line.strip_suffix('\r').unwrap_or(line) == DELIM {
            let yaml = &after_open[..offset];
            let body = if line_end < after_open.len() {
                &after_open[line_end + 1..]
            } else {
                ""
            };
            return Some((yaml, body));
        }
        if line_end >= after_open.len() {
            return None; // reached end of input with no closing delimiter
        }
        offset = line_end + 1;
    }
}

/// Parse a note into its [`FrontMatter`] and body.
///
/// A note is `"---\n<yaml>\n---\n<body>"`. When there is no leading
/// front-matter block the result is [`FrontMatter::default`] and the body
/// is the whole string verbatim. Malformed YAML *inside* a present
/// delimiter block is an [`Error::Yaml`].
pub fn parse(note: &str) -> Result<(FrontMatter, String), Error> {
    match split(note) {
        Some((yaml, body)) => {
            // An empty block (`---\n---\n`) is just the default — YAML
            // would deserialize it as null, which the struct rejects.
            let fm = if yaml.trim().is_empty() {
                FrontMatter::default()
            } else {
                serde_yaml_ng::from_str(yaml).context(YamlSnafu)?
            };
            Ok((fm, body.to_owned()))
        }
        None => Ok((FrontMatter::default(), note.to_owned())),
    }
}

/// The note body with any front-matter block stripped. Structural and
/// infallible: even a malformed front-matter block is removed (it is still
/// metadata, just not parseable), so the body the content hash sees is
/// always free of front-matter.
pub fn body_without_frontmatter(note: &str) -> &str {
    match split(note) {
        Some((_, body)) => body,
        None => note,
    }
}

/// Render a [`FrontMatter`] and body back into a full note string. When the
/// front-matter is entirely default (no metadata) the body is returned
/// as-is — no empty `---` block is written.
fn render(fm: &FrontMatter, body: &str) -> Result<String, Error> {
    if *fm == FrontMatter::default() {
        return Ok(body.to_owned());
    }
    let yaml = serde_yaml_ng::to_string(fm).context(SerializeSnafu)?;
    // `serde_yaml_ng::to_string` already ends the block with a newline.
    Ok(format!("{DELIM}\n{yaml}{DELIM}\n{body}"))
}

/// Set (or update) the note title, returning the full note string with the
/// body preserved verbatim. Adds a front-matter block if the note has none.
pub fn set_title(note: &str, title: &str) -> Result<String, Error> {
    let (mut fm, body) = parse(note)?;
    fm.title = Some(title.to_owned());
    render(&fm, &body)
}

/// Canonical content hash of a note's body, the key every derived artifact
/// (title, tags, links, embeddings, graph edges) is stamped with so
/// staleness is detectable and self-healing.
///
/// The hash is sha256 (hex) over the **normalized body, excluding any
/// front-matter block**. Normalization, in order:
///
/// 1. Drop the front-matter block (so a metadata-only edit — retitling,
///    retagging — never changes the hash).
/// 2. Convert CRLF (and lone CR) line endings to LF.
/// 3. Strip trailing whitespace from each line.
/// 4. Strip trailing blank lines.
///
/// The consequence: cosmetic whitespace churn and metadata edits leave the
/// hash stable, while a change to the substantive body text changes it.
///
/// Two edges are deliberate, not oversights:
///
/// - Step 3 trims *all* trailing per-line whitespace, so a change confined
///   to trailing whitespace — including adding or removing a Markdown hard
///   line break (two trailing spaces) — is intentionally hash-invisible.
///   Derived artifacts are keyed to the prose, not to soft-break layout.
/// - Stripping the front-matter block is structural (see
///   [`body_without_frontmatter`]): a leading `---`-delimited block is
///   excluded even when its YAML does not parse, so authored prose that
///   merely *looks* like a leading block sits outside staleness detection.
///   Front-matter authors keep the opening block tight, so the trigger is
///   narrow.
pub fn content_hash(note: &str) -> String {
    let body = body_without_frontmatter(note);
    let normalized = normalize_body(body);
    to_hex(&Sha256::digest(normalized.as_bytes()))
}

/// Normalize a body for hashing: LF line endings, no trailing per-line
/// whitespace, no trailing blank lines. See [`content_hash`].
fn normalize_body(body: &str) -> String {
    let lf = body.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<&str> = lf.split('\n').map(str::trim_end).collect();
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

/// Lowercase hex encoding of a digest. (Avoids a `hex` dependency for the
/// one place that needs it.)
fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_frontmatter_yields_default_and_whole_body() {
        let note = "just a body\nwith two lines\n";
        let (fm, body) = parse(note).unwrap();
        assert_eq!(fm, FrontMatter::default());
        assert_eq!(body, note);
        assert_eq!(body_without_frontmatter(note), note);
    }

    #[test]
    fn parses_a_full_frontmatter_block() {
        let note = "---\ntitle: My Note\ntags:\n  - rust\n  - notes\naliases:\n  - scratch\n---\nthe body\n";
        let (fm, body) = parse(note).unwrap();
        assert_eq!(fm.title.as_deref(), Some("My Note"));
        assert_eq!(fm.tags, vec!["rust", "notes"]);
        assert_eq!(fm.aliases, vec!["scratch"]);
        assert_eq!(body, "the body\n");
    }

    #[test]
    fn partial_frontmatter_defaults_missing_fields() {
        let note = "---\ntitle: Only A Title\n---\nbody\n";
        let (fm, body) = parse(note).unwrap();
        assert_eq!(fm.title.as_deref(), Some("Only A Title"));
        assert!(fm.tags.is_empty());
        assert!(fm.aliases.is_empty());
        assert_eq!(body, "body\n");
    }

    #[test]
    fn empty_frontmatter_block_is_the_default() {
        let note = "---\n---\nbody\n";
        let (fm, body) = parse(note).unwrap();
        assert_eq!(fm, FrontMatter::default());
        assert_eq!(body, "body\n");
    }

    #[test]
    fn unknown_keys_are_preserved_not_dropped() {
        // Real Obsidian vaults carry keys this crate does not model
        // (`created`, `cssclasses`, custom keys). They must survive a
        // parse -> set_title -> parse round-trip, or the canonical title
        // writer would silently strip a note's other metadata.
        let note = "---\ntitle: T\ncreated: 2026-06-27\nfoo: bar\n---\nbody\n";
        let (fm, _) = parse(note).unwrap();
        assert_eq!(fm.title.as_deref(), Some("T"));
        assert!(fm.extra.contains_key("created"));
        assert!(fm.extra.contains_key("foo"));

        let retitled = set_title(note, "Renamed").unwrap();
        let (fm2, body2) = parse(&retitled).unwrap();
        assert_eq!(fm2.title.as_deref(), Some("Renamed"));
        // The foreign keys are still there after the retitle write-back…
        assert_eq!(
            fm2.extra.get("created").and_then(|v| v.as_str()),
            Some("2026-06-27")
        );
        assert_eq!(fm2.extra.get("foo").and_then(|v| v.as_str()), Some("bar"));
        // …and the body is untouched.
        assert_eq!(body2, "body\n");
    }

    #[test]
    fn extra_only_frontmatter_is_not_treated_as_default() {
        // A note whose only metadata is a foreign key must still render a
        // block (otherwise render's default short-circuit would drop it).
        let note = "---\ncreated: 2026-06-27\n---\nbody\n";
        let (fm, body) = parse(note).unwrap();
        assert!(fm.title.is_none());
        assert_ne!(fm, FrontMatter::default());
        let rendered = render(&fm, &body).unwrap();
        let (fm2, _) = parse(&rendered).unwrap();
        assert_eq!(
            fm2.extra.get("created").and_then(|v| v.as_str()),
            Some("2026-06-27")
        );
    }

    #[test]
    fn malformed_yaml_inside_delimiters_errors() {
        let note = "---\nthis is: : not yaml\n---\nbody\n";
        let err = parse(note).unwrap_err();
        assert!(matches!(err, Error::Yaml { .. }));
        assert!(err.to_string().contains("malformed YAML"));
    }

    #[test]
    fn render_round_trips_losslessly() {
        let note = "---\ntitle: Round Trip\ntags:\n  - a\n  - b\n---\nbody line 1\nbody line 2\n";
        let (fm, body) = parse(note).unwrap();
        let rendered = render(&fm, &body).unwrap();
        let (fm2, body2) = parse(&rendered).unwrap();
        assert_eq!(fm, fm2);
        assert_eq!(body, body2);
    }

    #[test]
    fn crlf_frontmatter_is_handled() {
        let note = "---\r\ntitle: Windows\r\n---\r\nbody\r\nmore\r\n";
        let (fm, body) = parse(note).unwrap();
        assert_eq!(fm.title.as_deref(), Some("Windows"));
        assert_eq!(body, "body\r\nmore\r\n");
    }

    #[test]
    fn body_lines_starting_with_dashes_are_not_delimiters() {
        let note = "---\ntitle: T\n---\nintro\n--- not a delimiter ---\noutro\n";
        let (fm, body) = parse(note).unwrap();
        assert_eq!(fm.title.as_deref(), Some("T"));
        assert_eq!(body, "intro\n--- not a delimiter ---\noutro\n");
    }

    #[test]
    fn a_bare_horizontal_rule_in_the_body_is_the_closing_delimiter() {
        // A `---` line *is* a valid closing delimiter; the first one after
        // the opening wins. Front-matter authors keep the block tight.
        let note = "---\ntitle: T\n---\nbody before\n---\nbody after\n";
        let (fm, body) = parse(note).unwrap();
        assert_eq!(fm.title.as_deref(), Some("T"));
        assert_eq!(body, "body before\n---\nbody after\n");
    }

    #[test]
    fn unicode_body_and_title_survive() {
        let note = "---\ntitle: 日本語のメモ\n---\né à ü 🦀 — em–dash\n";
        let (fm, body) = parse(note).unwrap();
        assert_eq!(fm.title.as_deref(), Some("日本語のメモ"));
        assert_eq!(body, "é à ü 🦀 — em–dash\n");
        // And the hash is well-formed hex over the unicode body.
        assert_eq!(content_hash(note).len(), 64);
    }

    #[test]
    fn hash_is_stable_across_irrelevant_whitespace() {
        let a = "first line\nsecond line\n";
        let b = "first line   \r\nsecond line\t\n\n\n"; // CRLF, trailing ws, trailing blanks
        assert_eq!(content_hash(a), content_hash(b));
    }

    #[test]
    fn hash_changes_on_a_body_edit() {
        let a = "the original body\n";
        let b = "the original body, edited\n";
        assert_ne!(content_hash(a), content_hash(b));
    }

    #[test]
    fn hash_is_unchanged_by_a_metadata_only_edit() {
        let before = "---\ntitle: Before\ntags:\n  - x\n---\nthe body stays put\n";
        let after = set_title(before, "After Retitling").unwrap();
        // The title changed…
        assert_eq!(
            parse(&after).unwrap().0.title.as_deref(),
            Some("After Retitling")
        );
        // …but the content hash did not.
        assert_eq!(content_hash(before), content_hash(after.as_str()));
    }

    #[test]
    fn hash_ignores_a_malformed_but_delimited_block() {
        // A structurally-present-but-unparsable block is still stripped
        // before hashing, so the hash equals the same body with no block.
        let malformed = "---\nthis is: : not yaml\n---\nthe body\n";
        let bare = "the body\n";
        assert_eq!(content_hash(malformed), content_hash(bare));
    }

    #[test]
    fn hash_matches_with_and_without_frontmatter_for_the_same_body() {
        let bare = "the same body\n";
        let with_fm = "---\ntitle: T\n---\nthe same body\n";
        assert_eq!(content_hash(bare), content_hash(with_fm));
    }

    #[test]
    fn set_title_adds_a_block_to_a_bare_note() {
        let note = "no front-matter here\n";
        let updated = set_title(note, "Fresh Title").unwrap();
        let (fm, body) = parse(&updated).unwrap();
        assert_eq!(fm.title.as_deref(), Some("Fresh Title"));
        assert_eq!(body, note);
    }

    #[test]
    fn set_title_updates_an_existing_title_and_keeps_other_fields() {
        let note = "---\ntitle: Old\ntags:\n  - keep\n---\nbody\n";
        let updated = set_title(note, "New").unwrap();
        let (fm, body) = parse(&updated).unwrap();
        assert_eq!(fm.title.as_deref(), Some("New"));
        assert_eq!(fm.tags, vec!["keep"]);
        assert_eq!(body, "body\n");
    }

    #[test]
    fn set_title_round_trips() {
        let note = "---\ntitle: A\n---\nbody\n";
        let once = set_title(note, "B").unwrap();
        let twice = set_title(&once, "B").unwrap();
        assert_eq!(once, twice);
    }
}
