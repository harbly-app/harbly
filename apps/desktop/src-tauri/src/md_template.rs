//! Self-contained HTML shell for rendered Markdown. The output is fed to the
//! thumbnail pipeline (a hidden WebView snapshots it), so the styling is a fixed
//! light "paper" look — matching how standalone HTML assets are captured on white,
//! independent of the app's light/dark appearance.

/// Wrap a rendered Markdown body fragment in a full, styled HTML document.
/// `lang` is the app UI language, used only for the `lang` attribute (affects
/// CJK line-breaking and font selection); it is sanitized to a safe token.
pub fn render_page(body_html: &str, lang: &str) -> String {
    let lang = sanitize_lang(lang);
    format!(
        "<!doctype html><html lang=\"{lang}\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<style>{STYLE}</style></head><body><article class=\"doc\">{body_html}</article></body></html>"
    )
}

/// Keep only ASCII letters, digits and '-' from the language tag so it can never
/// break out of the attribute.
fn sanitize_lang(lang: &str) -> String {
    let s: String = lang
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(12)
        .collect();
    if s.is_empty() {
        "en".to_string()
    } else {
        s
    }
}

const STYLE: &str = r#"
:root { color-scheme: light; }
* { box-sizing: border-box; }
html, body { margin: 0; background: #ffffff; }
body {
  color: #211f26;
  font-family: -apple-system, BlinkMacSystemFont, "PingFang SC", "Noto Sans SC",
    "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
  font-size: 16px;
  line-height: 1.7;
  -webkit-font-smoothing: antialiased;
}
.doc {
  max-width: 46rem;
  margin: 0 auto;
  padding: 44px 52px 72px;
}
.doc > :first-child { margin-top: 0; }
h1, h2, h3, h4, h5, h6 {
  font-weight: 680;
  line-height: 1.3;
  margin: 1.8em 0 0.6em;
  color: #17151c;
}
h1 { font-size: 1.9em; margin-top: 0.2em; }
h2 { font-size: 1.5em; padding-bottom: 0.24em; border-bottom: 1px solid #eceaf2; }
h3 { font-size: 1.24em; }
h4 { font-size: 1.06em; }
h5, h6 { font-size: 0.94em; color: #4b4754; }
p { margin: 0 0 1em; }
a { color: #6e56cf; text-decoration: none; }
a:hover { text-decoration: underline; }
ul, ol { margin: 0 0 1em; padding-left: 1.6em; }
li { margin: 0.25em 0; }
li > ul, li > ol { margin: 0.25em 0; }
ul.contains-task-list { list-style: none; padding-left: 0.2em; }
li.task-list-item { display: flex; align-items: flex-start; gap: 0.5em; }
li.task-list-item input { margin-top: 0.42em; }
blockquote {
  margin: 0 0 1em;
  padding: 0.2em 1.1em;
  border-left: 3px solid #d9d5e6;
  color: #6b6676;
}
blockquote > :last-child { margin-bottom: 0; }
code {
  font-family: "SF Mono", "JetBrains Mono", Menlo, Monaco, Consolas, monospace;
  font-size: 0.88em;
  background: #f4f3f8;
  padding: 0.16em 0.4em;
  border-radius: 6px;
}
pre {
  background: #f7f6fb;
  border: 1px solid #eceaf2;
  border-radius: 12px;
  padding: 16px 18px;
  overflow-x: auto;
  margin: 0 0 1em;
  line-height: 1.55;
}
pre code { background: none; padding: 0; font-size: 0.86em; }
img { max-width: 100%; border-radius: 8px; }
hr { border: none; border-top: 1px solid #eceaf2; margin: 2em 0; }
table {
  border-collapse: collapse;
  width: 100%;
  margin: 0 0 1em;
  font-size: 0.95em;
  overflow: hidden;
}
th, td { border: 1px solid #eceaf2; padding: 0.5em 0.8em; text-align: left; }
th { background: #f7f6fb; font-weight: 640; }
tr:nth-child(even) td { background: #fbfafd; }
kbd {
  font-family: inherit;
  font-size: 0.82em;
  background: #f4f3f8;
  border: 1px solid #e2dfec;
  border-bottom-width: 2px;
  border-radius: 6px;
  padding: 0.1em 0.4em;
}
sup { line-height: 0; }
.footnote-definition { font-size: 0.9em; color: #6b6676; }
"#;
