//! Markdown handling: metadata/body extraction for the index, and rendering to
//! an HTML fragment for thumbnail generation. Parsing goes through pulldown-cmark
//! with GitHub-flavored extensions (tables, task lists, strikethrough, footnotes)
//! plus YAML front matter blocks.

use crate::extract::{truncate_on_char_boundary, Extracted, BODY_CAP};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use pulldown_cmark::{CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
}

/// Extract a title and tag-stripped body text from Markdown for indexing.
/// Title precedence: front-matter `title:` > first level-1 heading > None
/// (the caller then falls back to the file stem, mirroring the HTML path).
pub(crate) fn extract_md(src: &str) -> Extracted {
    let mut fm_title: Option<String> = None;
    let mut h1_title: Option<String> = None;
    let mut body = String::new();
    let mut in_meta = false;
    let mut in_h1 = false;
    let mut h1_buf = String::new();

    for ev in Parser::new_ext(src, options()) {
        match ev {
            Event::Start(Tag::MetadataBlock(_)) => in_meta = true,
            Event::End(TagEnd::MetadataBlock(_)) => in_meta = false,
            Event::Start(Tag::Heading {
                level: HeadingLevel::H1,
                ..
            }) if h1_title.is_none() => {
                in_h1 = true;
                h1_buf.clear();
            }
            Event::End(TagEnd::Heading(HeadingLevel::H1)) if in_h1 => {
                in_h1 = false;
                let t = h1_buf.split_whitespace().collect::<Vec<_>>().join(" ");
                if !t.is_empty() {
                    h1_title = Some(t);
                }
            }
            Event::Text(t) | Event::Code(t) => {
                if in_meta {
                    for line in t.lines() {
                        if let Some(v) = parse_front_matter_title(line) {
                            fm_title = Some(v);
                        }
                    }
                } else {
                    if in_h1 {
                        h1_buf.push_str(&t);
                    }
                    if body.len() < BODY_CAP {
                        body.push_str(t.trim());
                        body.push(' ');
                    }
                }
            }
            _ => {}
        }
    }
    truncate_on_char_boundary(&mut body, BODY_CAP);
    Extracted {
        title: fm_title.or(h1_title),
        body,
    }
}

/// Parse a single front-matter line for `title: ...`, tolerating surrounding
/// quotes. Deliberately line-based rather than a full YAML parse: only the title
/// is needed, and the front matter is otherwise preserved verbatim on the frontend.
fn parse_front_matter_title(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix("title:")?;
    let v = rest.trim().trim_matches('"').trim_matches('\'').trim();
    (!v.is_empty()).then(|| v.to_string())
}

/// Render Markdown to an HTML body fragment. When `rel_base` is set, relative
/// image URLs are rewritten to `{rel_base}{percent-encoded path}` so sibling
/// images resolve through the asset protocol (the sandbox CSP forbids a `<base>`
/// element, so the rewrite happens here in the event stream instead).
pub fn md_to_html_body(src: &str, rel_base: Option<&str>) -> String {
    let mapped = Parser::new_ext(src, options()).map(|ev| match ev {
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            let dest_url = match rel_base {
                Some(base) if is_relative_url(&dest_url) => {
                    CowStr::from(format!("{base}{}", encode_rel_path(&dest_url)))
                }
                _ => dest_url,
            };
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            })
        }
        // Raw HTML passes through to the rendered page, where CSP constrains
        // subresources but NOT navigation: a `<meta http-equiv="refresh">`
        // would send the (hidden thumbnail / preview) page to an arbitrary
        // URL with zero interaction — network egress the sandbox promises to
        // block. Meta tags have no legitimate use inside a Markdown body, so
        // any chunk containing one is rendered as visible text instead.
        Event::Html(h) if contains_meta_tag(&h) => Event::Text(h),
        Event::InlineHtml(h) if contains_meta_tag(&h) => Event::Text(h),
        other => other,
    });
    let mut out = String::new();
    pulldown_cmark::html::push_html(&mut out, mapped);
    out
}

/// Case-insensitive check for a `<meta` opening tag in a raw-HTML chunk.
/// Chunk-level (not tag-level) on purpose: an HTML block arrives line by line,
/// and a tag split across lines ("<meta\nhttp-equiv=…") must still be caught —
/// escaping the line that opens the tag prevents it from ever forming. The
/// character AFTER "<meta" must end the tag name (whitespace, '>', '/', or
/// the chunk boundary), so SVG's legitimate `<metadata>` element passes.
fn contains_meta_tag(h: &str) -> bool {
    let lower = h.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find("<meta") {
        let rest = &lower[from + i + 5..];
        match rest.chars().next() {
            None => return true, // tag opens at the chunk boundary
            Some(c) if c.is_ascii_whitespace() || c == '>' || c == '/' => return true,
            _ => from += i + 5,
        }
    }
    false
}

/// A URL that should resolve relative to the document (no scheme, not absolute).
fn is_relative_url(u: &str) -> bool {
    !u.is_empty()
        && !u.starts_with('/')
        && !u.starts_with('#')
        && !u.contains("://")
        && !u.starts_with("data:")
        && !u.starts_with("mailto:")
}

/// Characters that must be percent-encoded within a single path segment.
const SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

/// Percent-encode each path segment while keeping '/' separators, so the protocol
/// handler can percent-decode segment by segment.
fn encode_rel_path(p: &str) -> String {
    p.split('/')
        .map(|s| utf8_percent_encode(s, SEGMENT).to_string())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::md_to_html_body;

    #[test]
    fn meta_refresh_is_neutralized_to_text() {
        let out = md_to_html_body(
            "before\n\n<meta http-equiv=\"refresh\" content=\"0;url=https://evil/?leak\">\n\nafter",
            None,
        );
        assert!(!out.contains("<meta"), "meta tag must not survive: {out}");
        assert!(
            out.contains("&lt;meta"),
            "must render as escaped text: {out}"
        );

        // Case variants and a tag split across HTML-block lines
        let out = md_to_html_body("<META HTTP-EQUIV=refresh content=1>", None);
        assert!(!out.to_ascii_lowercase().contains("<meta"));
        let out = md_to_html_body("<meta\nhttp-equiv=\"refresh\" content=\"0;url=x\">", None);
        assert!(!out.to_ascii_lowercase().contains("<meta"));

        // Inline position too
        let out = md_to_html_body("text <meta http-equiv=refresh content=1> more", None);
        assert!(!out.to_ascii_lowercase().contains("<meta"));
    }

    #[test]
    fn benign_raw_html_still_passes_through() {
        let out = md_to_html_body(
            "a <kbd>⌘K</kbd> b\n\n<details><summary>t</summary>x</details>",
            None,
        );
        assert!(out.contains("<kbd>"));
        assert!(out.contains("<details>"));

        // SVG's <metadata> element must not trip the <meta detector — an
        // escaped chunk would corrupt the whole inline SVG.
        let svg =
            "<svg viewBox=\"0 0 1 1\"><metadata>m</metadata><rect width=\"1\" height=\"1\"/></svg>";
        let out = md_to_html_body(svg, None);
        assert!(
            out.contains("<metadata>"),
            "metadata must pass through: {out}"
        );
    }
}
