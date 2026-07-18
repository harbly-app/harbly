//! Harbly page documents (.hdoc): a constrained custom-element vocabulary over
//! plain HTML. The file on disk is the clean content itself (`<h-doc>` root with
//! `h-*` component tags plus a whitelist of native tags); the shell layer wraps
//! it in a styling/behavior runtime at render time, the same way Markdown is
//! rendered through a template. This module only handles what the index needs:
//! title/body extraction for FTS.

use crate::error::{HarblyError, Result};
use crate::extract::{truncate_on_char_boundary, Extracted, BODY_CAP};
use scraper::{Html, Selector};

/// Vocabulary version stamped on new documents (`<h-doc v="1">`). Once
/// documents exist in user libraries this is a forever contract: bump only for
/// additive spec changes, and the renderer keeps accepting every older value.
pub const HDOC_VOCAB_VERSION: &str = "1";

/// Skeleton written by "New page": empty title + paragraph in the default theme.
pub const HDOC_NEW_TEMPLATE: &str =
    "<h-doc v=\"1\" theme=\"paper\">\n  <h1></h1>\n  <p></p>\n</h-doc>\n";

/// Component attributes that carry user-visible text (callout/card/step titles,
/// stat values, figure captions, quote attributions …) — indexed for search.
const TEXT_ATTRS: [&str; 6] = ["title", "caption", "summary", "label", "value", "cite"];

/// Extract a title and searchable body text from an hdoc source.
/// Title = the first `<h1>`'s text; the caller falls back to the file stem
/// (mirroring the HTML and Markdown paths).
pub(crate) fn extract_hdoc(content: &str) -> Extracted {
    let doc = Html::parse_document(content);

    let title = Selector::parse("h1")
        .ok()
        .and_then(|sel| doc.select(&sel).next())
        .map(|el| {
            el.text()
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|t| !t.is_empty());

    let mut body = String::new();
    walk(doc.tree.root(), &mut body);
    truncate_on_char_boundary(&mut body, BODY_CAP);
    Extracted { title, body }
}

/// Collect indexable text: element text plus the user-visible attributes of
/// h-* components. Iterative (an explicit pre/post traversal, not call-stack
/// recursion) for the same reason as extract.rs walk — a deeply nested document
/// must not overflow the stack and crash the scan, re-crashing on every rescan.
fn walk(root: ego_tree::NodeRef<scraper::Node>, out: &mut String) {
    let mut skip_depth = 0usize;
    // Scripts never execute in an hdoc (CSP), but their source text still must
    // not pollute the index.
    let is_skipped = |el: &scraper::node::Element| {
        matches!(el.name(), "script" | "style" | "noscript" | "template")
    };
    for edge in root.traverse() {
        if out.len() >= BODY_CAP {
            break;
        }
        match edge {
            ego_tree::iter::Edge::Open(node) => match node.value() {
                scraper::Node::Element(el) if is_skipped(el) => skip_depth += 1,
                scraper::Node::Element(el) if skip_depth == 0 && el.name().starts_with("h-") => {
                    for a in TEXT_ATTRS {
                        if let Some(v) = el.attr(a) {
                            let v = v.trim();
                            if !v.is_empty() {
                                out.push_str(v);
                                out.push(' ');
                            }
                        }
                    }
                }
                scraper::Node::Text(t) if skip_depth == 0 => {
                    let tx = t.trim();
                    if !tx.is_empty() {
                        out.push_str(tx);
                        out.push(' ');
                    }
                }
                _ => {}
            },
            ego_tree::iter::Edge::Close(node) => {
                if let scraper::Node::Element(el) = node.value() {
                    if is_skipped(el) {
                        skip_depth = skip_depth.saturating_sub(1);
                    }
                }
            }
        }
    }
}

/// How an element tag relates to the v1 vocabulary.
enum TagClass {
    /// html/head/body — inserted by the HTML parser, never part of the content;
    /// tolerated only bare (any attribute, e.g. `<body onload>`, is a smuggled
    /// handler and must be refused).
    Wrapper,
    /// A vocabulary tag and the exact attribute names it may carry.
    Vocab(&'static [&'static str]),
    /// Anything else (script/style/iframe/div/unknown custom element).
    Unknown,
}

/// The v1 tag + attribute whitelist, kept in lockstep with the editor's
/// schema.ts (ALLOWED_TAGS / ALLOWED_ATTRS) — that file is the source of truth.
fn classify(tag: &str) -> TagClass {
    use TagClass::*;
    match tag {
        "html" | "head" | "body" => Wrapper,
        "h-doc" => Vocab(&["v", "theme", "layout"]),
        "h-callout" => Vocab(&["kind", "title"]),
        "h-card" => Vocab(&["title"]),
        "h-step" => Vocab(&["title"]),
        "h-figure" => Vocab(&["width", "align"]),
        "h-quote" => Vocab(&["cite"]),
        "h-stat" => Vocab(&["value", "label"]),
        "h-details" => Vocab(&["summary", "open"]),
        "img" => Vocab(&["src", "alt"]),
        "a" => Vocab(&["href"]),
        "th" | "td" => Vocab(&["colspan", "rowspan"]),
        "h-columns" | "h-column" | "h-steps" | "h-stats" | "h-toc" | "p" | "h1" | "h2" | "h3"
        | "ul" | "ol" | "li" | "blockquote" | "pre" | "code" | "hr" | "br" | "strong" | "b"
        | "em" | "i" | "s" | "del" | "table" | "thead" | "tbody" | "tr" => Vocab(&[]),
        _ => Unknown,
    }
}

/// Reject an hdoc source that strays outside the v1 vocabulary — the same
/// tag + attribute whitelist the editor enforces before it will edit a file.
///
/// The in-app render neuters a stray `<script>` with a nonce CSP, but "Export
/// as HTML" and browser preview bake the source into a standalone file with NO
/// CSP, so anything executable (a script element, an `onclick`/`onerror`
/// handler, a smuggled `<body onload>`, a `javascript:` URL in `href`/`src`)
/// has to be refused up front rather than silently stripped. Validates the
/// whole parsed document, not just the h-doc subtree, so nothing can hide in
/// a wrapper the baker still emits.
pub fn validate_hdoc_vocabulary(source: &str) -> Result<()> {
    let doc = Html::parse_document(source);
    let mut has_root = false;
    for node in doc.tree.nodes() {
        let Some(el) = node.value().as_element() else {
            continue;
        };
        let name = el.name();
        if name == "h-doc" {
            has_root = true;
        }
        match classify(name) {
            TagClass::Wrapper => {
                if el.attrs().next().is_some() {
                    return Err(reject());
                }
            }
            TagClass::Vocab(allowed) => {
                for (attr, value) in el.attrs() {
                    if !allowed.contains(&attr) {
                        return Err(reject());
                    }
                    let value_ok = match (name, attr) {
                        ("a", "href") => safe_url(value, false),
                        ("img", "src") => safe_url(value, true),
                        _ => true,
                    };
                    if !value_ok {
                        return Err(reject());
                    }
                }
            }
            TagClass::Unknown => return Err(reject()),
        }
    }
    if !has_root {
        return Err(reject());
    }
    Ok(())
}

/// Whether a URL attribute value is safe to bake into a CSP-less export:
/// http(s)/mailto, fragment anchors, and relative paths — plus `data:image/*`
/// where `allow_data_image` (img src). Everything else — javascript:, vbscript:,
/// non-image data:, file:, unknown schemes — is refused. The scheme is checked
/// on a normalized copy (ASCII whitespace/control characters stripped, then
/// lowercased) because HTML URL parsing tolerates them inside the scheme —
/// "java\tscript:" still executes in a browser.
fn safe_url(value: &str, allow_data_image: bool) -> bool {
    let normalized: String = value
        .chars()
        .filter(|c| !c.is_ascii_whitespace() && !c.is_ascii_control())
        .collect::<String>()
        .to_ascii_lowercase();
    let Some(colon) = normalized.find(':') else {
        return true; // no scheme → relative path or #fragment
    };
    // A colon past the first path/query/fragment delimiter is data, not a
    // scheme separator ("a/b:c", "#t:1", "?x=a:b").
    if normalized[..colon]
        .chars()
        .any(|c| matches!(c, '/' | '?' | '#'))
    {
        return true;
    }
    match &normalized[..colon] {
        "http" | "https" | "mailto" => true,
        "data" => allow_data_image && normalized[colon + 1..].starts_with("image/"),
        _ => false,
    }
}

fn reject() -> HarblyError {
    HarblyError::msg("内容超出 hdoc 词汇表")
}

#[cfg(test)]
mod tests {
    use super::validate_hdoc_vocabulary as v;

    #[test]
    fn accepts_full_vocabulary_including_merged_cells() {
        assert!(v(r#"<h-doc v="1" theme="future">
            <h1>标题</h1>
            <p>正文 <strong>粗</strong> <a href="https://x">链</a></p>
            <h-callout kind="tip" title="t"><p>x</p></h-callout>
            <h-figure width="60" align="left"><img src="data:image/png;base64,AAAA" alt="a"></h-figure>
            <table>
              <tr><th colspan="2">头</th></tr>
              <tr><td rowspan="2">甲</td><td>乙</td></tr>
              <tr><td>丙</td></tr>
            </table>
        </h-doc>"#)
        .is_ok());
    }

    #[test]
    fn rejects_embedded_script_and_handlers() {
        assert!(v(r#"<h-doc v="1"><p>x</p><script>alert(1)</script></h-doc>"#).is_err());
        assert!(v(r#"<h-doc v="1"><p onclick="evil()">x</p></h-doc>"#).is_err());
        assert!(v(r#"<h-doc v="1"><img src="x" onerror="evil()"></h-doc>"#).is_err());
        assert!(v(r#"<h-doc v="1"><style>p{color:red}</style><p>x</p></h-doc>"#).is_err());
    }

    #[test]
    fn rejects_smuggled_wrapper_attributes() {
        // A <body onload> in the source would hoist onto the baked document body.
        assert!(v(r#"<body onload="evil()"><h-doc v="1"><p>x</p></h-doc></body>"#).is_err());
    }

    #[test]
    fn rejects_executable_url_values() {
        assert!(v(r#"<h-doc v="1"><p><a href="javascript:alert(1)">x</a></p></h-doc>"#).is_err());
        assert!(v(r#"<h-doc v="1"><p><a href="JaVaScRiPt:alert(1)">x</a></p></h-doc>"#).is_err());
        // Whitespace inside the scheme still executes in browsers
        assert!(
            v("<h-doc v=\"1\"><p><a href=\"java\tscript:alert(1)\">x</a></p></h-doc>").is_err()
        );
        assert!(v(r#"<h-doc v="1"><p><a href="data:text/html,x">x</a></p></h-doc>"#).is_err());
        assert!(v(r#"<h-doc v="1"><img src="javascript:1" alt="a"></h-doc>"#).is_err());
        assert!(v(r#"<h-doc v="1"><img src="data:text/html,x" alt="a"></h-doc>"#).is_err());
    }

    #[test]
    fn accepts_benign_url_values() {
        assert!(v(r##"<h-doc v="1"><p>
            <a href="https://example.com/a?q=1#f">x</a>
            <a href="mailto:a@b.c">m</a>
            <a href="#sec">anchor</a>
            <a href="../other.html">rel</a>
            <a href="page.html#t:1">colon in fragment</a>
        </p></h-doc>"##)
        .is_ok());
        assert!(v(r#"<h-doc v="1">
            <h-figure><img src="images/a.png" alt=""></h-figure>
            <h-figure><img src="data:image/png;base64,AA" alt=""></h-figure>
        </h-doc>"#)
        .is_ok());
    }

    #[test]
    fn rejects_unknown_tags_and_missing_root() {
        assert!(v(r#"<h-doc v="1"><h-chart></h-chart></h-doc>"#).is_err());
        assert!(v(r#"<h-doc v="1"><div><p>x</p></div></h-doc>"#).is_err());
        assert!(v(r#"<p>no root</p>"#).is_err());
    }
}
