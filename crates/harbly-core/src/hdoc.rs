//! Harbly page documents (.hdoc): a constrained custom-element vocabulary over
//! plain HTML. The file on disk is the clean content itself (`<h-doc>` root with
//! `h-*` component tags plus a whitelist of native tags); the shell layer wraps
//! it in a styling/behavior runtime at render time, the same way Markdown is
//! rendered through a template. This module only handles what the index needs:
//! title/body extraction for FTS.

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

fn walk(node: ego_tree::NodeRef<scraper::Node>, out: &mut String) {
    if out.len() >= BODY_CAP {
        return;
    }
    for child in node.children() {
        match child.value() {
            scraper::Node::Element(el) => {
                // Scripts never execute in an hdoc (CSP), but their source text
                // still must not pollute the index.
                if matches!(el.name(), "script" | "style" | "noscript" | "template") {
                    continue;
                }
                if el.name().starts_with("h-") {
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
                walk(child, out);
            }
            scraper::Node::Text(t) => {
                let tx = t.trim();
                if !tx.is_empty() {
                    out.push_str(tx);
                    out.push(' ');
                }
            }
            _ => {}
        }
    }
}
