//! Markdown → HTML for the reading view, native-tested off the browser.
//!
//! The note body (front-matter stripped, since it is chrome not content) is
//! rendered with `pulldown-cmark`. Two transforms ride on top of the plain
//! CommonMark/GFM pass, both implemented as a rewrite of the parser's event
//! stream so they compose with normal Markdown structure rather than
//! string-munging the source:
//!
//! - **`[[wikilinks]]`** become clickable links that switch notes — an
//!   `<a href="?note=<target>">` the shell follows like any other in-page
//!   nav. `[[target|display]]` uses `display` as the link text. Wikilinks
//!   inside code spans / fenced code are left verbatim.
//! - **Safe rendering.** Raw HTML embedded in a note is *not* passed
//!   through — `Html`/`InlineHtml` events are re-emitted as text so they are
//!   escaped, and link/image destinations carrying a dangerous scheme
//!   (`javascript:`, `data:`, …) are dropped. A note can therefore never
//!   inject script or active markup into the reading view.

use notes_protocol::frontmatter;
use pulldown_cmark::{html, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd};

/// Render a full note (front-matter included) to sanitized reading-view
/// HTML. The front-matter block is dropped; the body is parsed as GFM with
/// the event-stream rewrites described in the module docs.
pub fn render_to_html(note: &str) -> String {
    let body = frontmatter::body_without_frontmatter(note);
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    let events = rewrite(Parser::new_ext(body, options));
    let mut out = String::new();
    html::push_html(&mut out, events.into_iter());
    out
}

/// Rewrite the parser event stream: neutralize raw HTML, sanitize link and
/// image destinations, and expand `[[wikilinks]]` in ordinary text.
fn rewrite<'a>(parser: impl Iterator<Item = Event<'a>>) -> Vec<Event<'a>> {
    let mut out = Vec::new();
    // Depth of code blocks we are inside; wikilink expansion is suppressed
    // within code so `[[not a link]]` in a snippet renders verbatim.
    let mut code_depth = 0u32;
    // Depth of Markdown links we are inside; wikilink expansion is suppressed
    // within link text so `[see [[note]]](url)` keeps the `[[note]]` literal
    // rather than nesting an `<a>` inside the outer link (invalid HTML).
    let mut link_depth = 0u32;
    // Consecutive `Text` events are coalesced here before wikilink expansion.
    // pulldown-cmark tokenizes `[[target]]` into separate single-character
    // `Text` events (`[`, `[`, `target`, `]`, `]`) because `[`/`]` are link
    // syntax, so a per-event scan would never see the `[[`/`]]` delimiters.
    // Joining the run first reconstructs the source span. Any non-`Text`
    // event flushes the buffer, so a code span or inline markup bounds a
    // wikilink rather than being swallowed into it.
    let mut text = String::new();
    for event in parser {
        // A non-text event ends the current text run: expand and flush it
        // before the event itself is emitted so output order is preserved.
        if !matches!(event, Event::Text(_)) {
            flush_text(&mut text, &mut out);
        }
        match event {
            // Raw HTML from the note is escaped, never passed through: an
            // `Html` block or `InlineHtml` span is re-emitted as text, which
            // `push_html` HTML-escapes. This is the core XSS guard.
            Event::Html(s) | Event::InlineHtml(s) => out.push(Event::Text(s)),
            Event::Start(Tag::CodeBlock(kind)) => {
                code_depth += 1;
                out.push(Event::Start(Tag::CodeBlock(kind)));
            }
            Event::End(TagEnd::CodeBlock) => {
                code_depth = code_depth.saturating_sub(1);
                out.push(Event::End(TagEnd::CodeBlock));
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                link_depth += 1;
                out.push(Event::Start(Tag::Link {
                    link_type,
                    dest_url: sanitize_url(dest_url),
                    title,
                    id,
                }));
            }
            Event::End(TagEnd::Link) => {
                link_depth = link_depth.saturating_sub(1);
                out.push(Event::End(TagEnd::Link));
            }
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => out.push(Event::Start(Tag::Image {
                link_type,
                dest_url: sanitize_url(dest_url),
                title,
                id,
            })),
            // Inline code is left as-is — no wikilink expansion inside it.
            Event::Code(_) => out.push(event),
            // Body text outside code and outside a link: accumulate the run
            // for wikilink expansion at the next flush.
            Event::Text(t) if code_depth == 0 && link_depth == 0 => text.push_str(&t),
            // Text inside a fenced/indented code block or a Markdown link:
            // emit verbatim so `[[not a link]]` stays literal and we never
            // nest an `<a>` inside an outer link.
            Event::Text(_) => out.push(event),
            other => out.push(other),
        }
    }
    // Flush any trailing text run (a document ending in plain text).
    flush_text(&mut text, &mut out);
    out
}

/// Expand any `[[wikilinks]]` in the accumulated `text` run, push the
/// resulting events, and clear the buffer. A no-op on an empty buffer.
fn flush_text<'a>(text: &mut String, out: &mut Vec<Event<'a>>) {
    if !text.is_empty() {
        let run = std::mem::take(text);
        expand_wikilinks(&run, out);
    }
}

/// Split a run of text on `[[…]]` wikilinks, pushing literal text and link
/// event triples in order. `[[target]]` links to and labels with `target`;
/// `[[target|display]]` labels with `display`. A `[[` with no closing `]]`,
/// or an empty target, is left as literal text.
fn expand_wikilinks<'a>(text: &str, out: &mut Vec<Event<'a>>) {
    let mut rest = text;
    while let Some(open) = rest.find("[[") {
        let after_open = &rest[open + 2..];
        let Some(close) = after_open.find("]]") else {
            break; // no closing delimiter — the remainder is literal
        };
        let inner = &after_open[..close];
        let (target, display) = match inner.split_once('|') {
            Some((t, d)) => (t.trim(), d.trim()),
            None => (inner.trim(), inner.trim()),
        };

        if target.is_empty() {
            // Keep the literal `[[…]]` (including the brackets) and continue
            // scanning after it.
            let through = open + 2 + close + 2;
            push_text(&rest[..through], out);
            rest = &rest[through..];
            continue;
        }

        push_text(&rest[..open], out);
        let href = format!("?note={}", encode_query(target));
        out.push(Event::Start(Tag::Link {
            link_type: LinkType::Inline,
            dest_url: CowStr::from(href),
            title: CowStr::from(""),
            id: CowStr::from(""),
        }));
        out.push(Event::Text(CowStr::from(display.to_owned())));
        out.push(Event::End(TagEnd::Link));
        rest = &rest[open + 2 + close + 2..];
    }
    push_text(rest, out);
}

/// Push `s` as a `Text` event unless it is empty.
fn push_text<'a>(s: &str, out: &mut Vec<Event<'a>>) {
    if !s.is_empty() {
        out.push(Event::Text(CowStr::from(s.to_owned())));
    }
}

/// Neutralize a link/image destination that carries a dangerous URL scheme.
/// Relative URLs (including our `?note=…` wikilinks) and the safe schemes
/// (`http`, `https`, `mailto`) pass through unchanged; anything with a
/// `javascript:`/`data:`/`vbscript:`/`file:` scheme is replaced with an
/// empty destination so a click does nothing.
fn sanitize_url(url: CowStr<'_>) -> CowStr<'_> {
    if is_safe_url(&url) {
        url
    } else {
        CowStr::from("")
    }
}

/// Whether `url` is safe to emit as a link/image destination. A URL with no
/// scheme is relative and safe (unless it is protocol-relative — see below); a
/// scheme is safe only if it is on the allowlist. Leading ASCII whitespace and
/// control characters (a common `javascript:` obfuscation) are ignored when
/// reading the scheme.
fn is_safe_url(url: &str) -> bool {
    let trimmed = url.trim_start_matches(|c: char| c.is_ascii_whitespace() || c.is_control());
    // A protocol-relative URL (`//evil.com`) carries no scheme but still
    // navigates off-origin, so it is rejected rather than treated as a
    // same-origin relative path.
    if trimmed.starts_with("//") {
        return false;
    }
    match trimmed.split_once(':') {
        None => true, // relative URL, no scheme
        Some((scheme, _)) => {
            // A `:` after a `/`, `?`, or `#` is part of a relative path or
            // query, not a scheme (e.g. `foo/bar:baz`, `?note=a:b`).
            if scheme.contains(['/', '?', '#']) {
                return true;
            }
            matches!(
                scheme.trim().to_ascii_lowercase().as_str(),
                "http" | "https" | "mailto"
            )
        }
    }
}

/// Percent-encode a wikilink target for the `?note=` query value. Note paths
/// are unreserved characters plus `/`; everything outside that set (spaces,
/// say) is percent-encoded so the href is well-formed. `/` is kept literal
/// so a nested path stays readable and the shell can split it.
fn encode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_basic_markdown_to_html() {
        let html = render_to_html("# Title\n\nSome **bold** and *italic* text.\n");
        assert!(html.contains("<h1>Title</h1>"));
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<em>italic</em>"));
    }

    #[test]
    fn front_matter_is_excluded_from_the_render() {
        let note =
            "---\ntitle: Hidden Chrome\ntags:\n  - secret\n---\n# Body Heading\n\nbody text\n";
        let html = render_to_html(note);
        assert!(html.contains("<h1>Body Heading</h1>"));
        // None of the front-matter keys/values leak into the rendered body.
        assert!(!html.contains("Hidden Chrome"));
        assert!(!html.contains("title:"));
        assert!(!html.contains("secret"));
    }

    #[test]
    fn renders_lists_and_code_blocks() {
        let note = "- one\n- two\n\n```\nlet x = 1;\n```\n";
        let html = render_to_html(note);
        assert!(html.contains("<ul>"));
        assert!(html.contains("<li>one</li>"));
        assert!(html.contains("<pre><code>"));
        assert!(html.contains("let x = 1;"));
    }

    #[test]
    fn renders_gfm_tables_and_strikethrough() {
        let note = "| a | b |\n| - | - |\n| 1 | 2 |\n\n~~gone~~\n";
        let html = render_to_html(note);
        assert!(html.contains("<table>"));
        assert!(html.contains("<del>gone</del>"));
    }

    #[test]
    fn a_plain_wikilink_becomes_a_note_switch_link() {
        let html = render_to_html("see [[projects/idea]] for more\n");
        assert!(
            html.contains(r#"<a href="?note=projects/idea">projects/idea</a>"#),
            "got: {html}"
        );
    }

    #[test]
    fn a_piped_wikilink_uses_the_display_text() {
        let html = render_to_html("see [[projects/idea|my idea]]\n");
        assert!(
            html.contains(r#"<a href="?note=projects/idea">my idea</a>"#),
            "got: {html}"
        );
    }

    #[test]
    fn a_wikilink_target_with_spaces_is_percent_encoded() {
        let html = render_to_html("[[Daily Note]]\n");
        assert!(
            html.contains(r#"<a href="?note=Daily%20Note">Daily Note</a>"#),
            "got: {html}"
        );
    }

    #[test]
    fn multiple_wikilinks_in_one_line_all_expand() {
        let html = render_to_html("[[a]] and [[b]]\n");
        assert!(html.contains(r#"<a href="?note=a">a</a>"#), "got: {html}");
        assert!(html.contains(r#"<a href="?note=b">b</a>"#), "got: {html}");
    }

    #[test]
    fn an_unclosed_wikilink_is_left_literal() {
        let html = render_to_html("text [[unterminated here\n");
        assert!(html.contains("[[unterminated here"), "got: {html}");
        assert!(!html.contains("<a "), "got: {html}");
    }

    #[test]
    fn an_empty_wikilink_target_is_left_literal() {
        let html = render_to_html("nothing [[]] here\n");
        assert!(html.contains("[[]]"), "got: {html}");
        assert!(!html.contains("<a "), "got: {html}");
    }

    #[test]
    fn wikilinks_inside_inline_code_are_not_expanded() {
        let html = render_to_html("use `[[literal]]` syntax\n");
        assert!(html.contains("<code>[[literal]]</code>"), "got: {html}");
        assert!(!html.contains("<a "), "got: {html}");
    }

    #[test]
    fn wikilinks_inside_fenced_code_are_not_expanded() {
        let html = render_to_html("```\n[[literal]]\n```\n");
        assert!(html.contains("[[literal]]"), "got: {html}");
        assert!(!html.contains("<a "), "got: {html}");
    }

    #[test]
    fn a_wikilink_inside_link_text_stays_literal() {
        // A `[[wikilink]]` written inside a normal Markdown link's text must
        // not expand into a nested `<a>` (invalid HTML); it stays literal and
        // only the outer link is emitted.
        let html = render_to_html("[see [[note]] here](https://x.com)\n");
        assert_eq!(html.matches("<a ").count(), 1, "nested anchor: {html}");
        assert!(
            html.contains(r#"<a href="https://x.com">"#),
            "outer link missing: {html}"
        );
        assert!(html.contains("[[note]]"), "wikilink not literal: {html}");
        assert!(!html.contains("?note=note"), "wikilink expanded: {html}");
    }

    #[test]
    fn raw_html_block_is_escaped_not_passed_through() {
        let html = render_to_html("<script>alert('xss')</script>\n");
        assert!(!html.contains("<script>"), "raw script leaked: {html}");
        assert!(html.contains("&lt;script&gt;"), "got: {html}");
    }

    #[test]
    fn raw_inline_html_is_escaped_not_passed_through() {
        let html = render_to_html("text with <img src=x onerror=alert(1)> inline\n");
        assert!(!html.contains("<img"), "raw img leaked: {html}");
        assert!(html.contains("&lt;img"), "got: {html}");
    }

    #[test]
    fn javascript_scheme_links_are_neutralized() {
        let html = render_to_html("[click](javascript:alert(1))\n");
        assert!(!html.contains("javascript:"), "js url leaked: {html}");
        // The link still renders, just with an inert destination.
        assert!(html.contains(r#"<a href="">click</a>"#), "got: {html}");
    }

    #[test]
    fn data_scheme_images_are_neutralized() {
        let html = render_to_html("![x](data:text/html;base64,PHNjcmlwdD4=)\n");
        assert!(!html.contains("data:text/html"), "data url leaked: {html}");
    }

    #[test]
    fn ordinary_http_links_are_preserved() {
        let html = render_to_html("[home](https://example.com/path)\n");
        assert!(
            html.contains(r#"<a href="https://example.com/path">home</a>"#),
            "got: {html}"
        );
    }

    #[test]
    fn is_safe_url_classifies_schemes() {
        assert!(is_safe_url("https://example.com"));
        assert!(is_safe_url("http://example.com"));
        assert!(is_safe_url("mailto:me@example.com"));
        assert!(is_safe_url("?note=foo/bar"));
        assert!(is_safe_url("relative/path"));
        assert!(is_safe_url("#anchor"));
        assert!(!is_safe_url("javascript:alert(1)"));
        assert!(!is_safe_url("  javascript:alert(1)"));
        assert!(!is_safe_url("java\tscript:alert(1)"));
        assert!(!is_safe_url("data:text/html,<x>"));
        assert!(!is_safe_url("vbscript:msgbox"));
        assert!(!is_safe_url("file:///etc/passwd"));
        // Protocol-relative URLs navigate off-origin and are rejected.
        assert!(!is_safe_url("//evil.com"));
        assert!(!is_safe_url("  //evil.com/path"));
    }

    #[test]
    fn encode_query_keeps_unreserved_and_slashes() {
        assert_eq!(encode_query("projects/idea"), "projects/idea");
        assert_eq!(encode_query("a-b_c.d~e"), "a-b_c.d~e");
        assert_eq!(encode_query("Daily Note"), "Daily%20Note");
        assert_eq!(encode_query("a&b=c"), "a%26b%3Dc");
    }
}
