use scraper::{Html, Selector};

pub struct Extracted {
    pub title: Option<String>,
    pub body: String,
}

const BODY_CAP: usize = 200_000;

/// Extract the <title> from HTML (falling back to the first h1) plus the tag-stripped body text (skipping script/style)
pub fn extract(content: &str) -> Extracted {
    let doc = Html::parse_document(content);

    let pick_text = |sel: &str| -> Option<String> {
        let s = Selector::parse(sel).ok()?;
        let el = doc.select(&s).next()?;
        let t = el.text().collect::<String>();
        let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
        if t.is_empty() {
            None
        } else {
            Some(t)
        }
    };
    let title = pick_text("title").or_else(|| pick_text("h1"));

    let mut body = String::new();
    walk(doc.tree.root(), &mut body, false);
    body.truncate(BODY_CAP);

    Extracted { title, body }
}

fn walk(node: ego_tree::NodeRef<scraper::Node>, out: &mut String, skip: bool) {
    if out.len() >= BODY_CAP {
        return;
    }
    for child in node.children() {
        match child.value() {
            scraper::Node::Element(el) => {
                let s = matches!(el.name(), "script" | "style" | "noscript" | "template");
                walk(child, out, skip || s);
            }
            scraper::Node::Text(t) if !skip => {
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
