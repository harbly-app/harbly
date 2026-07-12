//! Self-contained HTML shell for hdoc page documents. The .hdoc file on disk is
//! the clean `<h-doc>` fragment; this template wraps it with the component
//! runtime (styles + a tiny behavior script) exactly the way md_template wraps
//! rendered Markdown. Two consumers:
//!   - the asset protocol (preview/thumbnails): runtime script carries a CSP
//!     nonce and relative media resolve through the sibling-file route;
//!   - "Export as HTML": no nonce, no rewrite base — a baked, standalone file.

pub const RUNTIME_CSS: &str = include_str!("../assets/hdoc/runtime.css");
const RUNTIME_JS: &str = include_str!("../assets/hdoc/runtime.js");

pub struct HdocRender<'a> {
    /// App UI language for the `lang` attribute (CJK line-breaking / fonts).
    pub lang: &'a str,
    /// Localized label for generated tables of contents.
    pub toc_label: &'a str,
    /// Sibling-file route base for relative media; None for baked exports.
    pub rel_base: Option<&'a str>,
    /// CSP nonce for the runtime script; None for baked exports (no CSP there).
    pub nonce: Option<&'a str>,
}

/// Wrap raw hdoc source in a full HTML document with the runtime embedded.
/// The source goes in verbatim: scripts inside it never run (the CSP only
/// honors the runtime's nonce), so sanitizing here would only hurt fidelity.
pub fn render_page(source: &str, o: &HdocRender) -> String {
    let lang = crate::md_template::sanitize_lang(o.lang);
    let nonce_attr = o
        .nonce
        .map(|n| format!(" nonce=\"{n}\""))
        .unwrap_or_default();
    let mut boot = String::new();
    if let Some(b) = o.rel_base {
        boot.push_str(&format!("window.__HDOC_REL_BASE={};", js_str(b)));
    }
    boot.push_str(&format!("window.__HDOC_TOC_LABEL={};", js_str(o.toc_label)));
    format!(
        "<!doctype html><html lang=\"{lang}\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<style>{RUNTIME_CSS}</style></head><body>{source}\
<script{nonce_attr}>{boot}{RUNTIME_JS}</script></body></html>"
    )
}

/// JSON string literal — the safe way to embed arbitrary text in a script.
fn js_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}
