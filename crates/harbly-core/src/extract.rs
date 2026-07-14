use scraper::{Html, Selector};

pub struct Extracted {
    pub title: Option<String>,
    pub body: String,
}

pub(crate) const BODY_CAP: usize = 200_000;

/// Truncate a string to at most `max` bytes, rounding down to a UTF-8 char
/// boundary so multi-byte text (e.g. CJK straddling the cap) never panics.
pub(crate) fn truncate_on_char_boundary(s: &mut String, max: usize) {
    if s.len() <= max {
        return;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
}

/// Extract the <title> from HTML (falling back to the first h1) plus the tag-stripped body text (skipping script/style)
pub fn extract_html(content: &str) -> Extracted {
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
    walk(doc.tree.root(), &mut body);
    truncate_on_char_boundary(&mut body, BODY_CAP);

    Extracted { title, body }
}

/// Collect visible text, skipping script/style subtrees. Iterative (an explicit
/// pre/post traversal, not call-stack recursion) so a pathologically deep
/// document — thousands of nested tags — can never overflow the stack and crash
/// the scan. A crash here poisoned the whole library: it struck before the asset
/// row was written, so every rescan re-parsed the same file and re-crashed.
fn walk(root: ego_tree::NodeRef<scraper::Node>, out: &mut String) {
    let mut skip_depth = 0usize;
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

#[cfg(test)]
mod tests {
    use super::extract_html;

    #[test]
    fn deeply_nested_html_does_not_overflow_the_stack() {
        // Run on a deliberately small stack: a recursive walk would overflow it
        // at this nesting depth (and abort the process), while the iterative walk
        // extracts the innermost text unharmed. Keeps the parse cheap, unlike the
        // huge depth a default-size stack would need to make the point.
        let ok = std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(|| {
                let depth = 8_000;
                let html = format!(
                    "<html><body>{}needle{}</body></html>",
                    "<div>".repeat(depth),
                    "</div>".repeat(depth),
                );
                extract_html(&html).body.contains("needle")
            })
            .unwrap()
            .join()
            .unwrap();
        assert!(ok);
    }
}
