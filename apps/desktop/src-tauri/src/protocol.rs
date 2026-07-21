use crate::state::AppState;
use percent_encoding::percent_decode_str;
use std::borrow::Cow;
use std::time::Duration;
use tauri::{http, Manager, Runtime, UriSchemeContext};

/// Sandbox CSP: allow inline scripts/styles (the norm for AI-generated files),
/// while sealing off every network egress.
const SANDBOX_CSP: &str = "default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval'; \
    style-src 'unsafe-inline'; img-src data: blob:; font-src data:; media-src data: blob:; \
    connect-src 'none'; form-action 'none'; base-uri 'none'; object-src 'none'";

/// One-shot allow: permit network resource loading, but still block form
/// submission / top-level navigation hijacking
const RELAXED_CSP: &str = "default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval' https: http:; \
    style-src 'unsafe-inline' https: http:; img-src https: http: data: blob:; font-src https: http: data:; \
    connect-src https: http:; media-src https: http: data: blob:; form-action 'none'; base-uri 'none'; object-src 'none'";

/// Markdown is server-rendered to a document (not an app): scripts are disabled
/// entirely, inline styles are allowed for the template, and images/media resolve
/// only through the asset protocol's sibling-file route.
const MD_CSP: &str = "default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; \
    img-src harbly-asset: data: blob:; font-src harbly-asset: data:; media-src harbly-asset: data: blob:; \
    connect-src 'none'; form-action 'none'; base-uri 'none'; object-src 'none'";

/// hdoc render: the document is content, not an app — only the injected runtime
/// (identified by the per-response nonce) may run; media resolve through the
/// asset protocol's sibling-file route.
fn hdoc_csp(nonce: &str) -> String {
    format!(
        "default-src 'none'; script-src 'nonce-{nonce}'; style-src 'unsafe-inline'; \
        img-src harbly-asset: data: blob:; font-src harbly-asset: data:; \
        media-src harbly-asset: data: blob:; connect-src 'none'; form-action 'none'; \
        base-uri 'none'; object-src 'none'"
    )
}

/// Injected into every previewed page: (1) CSP-violation counter for the
/// "N blocked" pill; (2) app-shortcut forwarding — the preview iframe is
/// cross-origin, so once it has focus the host window would never see
/// ⌘J/⌘K/⌘B, ⌘±/⌘0 (zoom), Escape or arrow keydowns. Forwarding is one-way
/// and limited to benign UI actions (panel toggles, zoom, close, file
/// navigation), so a malicious page spoofing the message can at worst flip a
/// panel or switch files.
///
/// Escape and arrows: skipped while a native <dialog open> exists (its
/// Escape-cancel never sets defaultPrevented) and relayed via setTimeout(0)
/// with a defaultPrevented re-check, so ANY page handler — including
/// window-level ones registered after this script — can veto them. Arrows
/// additionally require that neither the root document nor the scroll
/// container under the last pointer press has anywhere to scroll (their
/// default action is then a no-op, so no preventDefault is needed), keeping
/// native arrow-key scrolling intact. Event targets go through composedPath
/// when available so shadow-DOM inputs/scrollers are seen, not their hosts.
const CSP_REPORTER: &str = concat!(
    "<script>(function(){var n=0;var mp=null;",
    "function relay(k){try{parent.postMessage({__harbly:'key',key:k},'*')}catch(_){}}",
    "function tgt(e){return (e.composedPath&&e.composedPath()[0])||e.target}",
    "addEventListener('securitypolicyviolation',function(e){n++;",
    "try{parent.postMessage({__harbly:'csp',count:n,uri:String(e.blockedURI||'')},'*')}catch(_){}});",
    "addEventListener('pointerdown',function(e){mp=tgt(e)},true);",
    "function innerScrollable(el){for(var x=el;x&&x.nodeType===1;x=x.parentElement||(x.getRootNode&&x.getRootNode().host)){",
    "var oy;try{oy=getComputedStyle(x).overflowY}catch(_){return true}",
    "if((oy==='auto'||oy==='scroll'||oy==='overlay')&&x.scrollHeight>x.clientHeight+1)return true}",
    "return false}",
    "addEventListener('keydown',function(e){var k=String(e.key||'').toLowerCase();",
    "if((e.metaKey||e.ctrlKey)&&!e.altKey&&(k==='j'||k==='k'||k==='b'||k==='='||k==='+'||k==='-'||k==='0')){",
    "e.preventDefault();relay(k);return}",
    "if(e.metaKey||e.ctrlKey||e.altKey||e.defaultPrevented)return;",
    "var t=tgt(e),tag=t&&t.tagName?String(t.tagName).toUpperCase():'';",
    "if(tag==='INPUT'||tag==='TEXTAREA'||tag==='SELECT'||(t&&t.isContentEditable))return;",
    "if(k!=='escape'&&k!=='arrowup'&&k!=='arrowdown')return;",
    "if(document.querySelector('dialog[open]'))return;",
    "if(k!=='escape'){var s=document.scrollingElement;",
    "if(!s||s.scrollHeight>s.clientHeight+1)return;",
    "if(mp&&innerScrollable(mp))return}",
    "setTimeout(function(){if(!e.defaultPrevented)relay(k)},0)",
    "})})();</script>"
);

fn resp(
    status: u16,
    ctype: &str,
    csp: Option<&str>,
    body: Vec<u8>,
) -> http::Response<Cow<'static, [u8]>> {
    let mut b = http::Response::builder()
        .status(status)
        .header("Content-Type", ctype)
        .header("Access-Control-Allow-Origin", "*")
        .header(
            "Cache-Control",
            if csp.is_some() {
                "no-store"
            } else {
                "public, max-age=31536000, immutable"
            },
        );
    if let Some(csp) = csp {
        b = b.header("Content-Security-Policy", csp);
    }
    b.body(Cow::Owned(body)).unwrap()
}

fn not_found() -> http::Response<Cow<'static, [u8]>> {
    resp(404, "text/plain", None, b"not found".to_vec())
}

/// Inject the CSP-violation counter script after <head> (or <html>); if neither
/// is found, prepend it
fn inject_reporter(html: Vec<u8>) -> Vec<u8> {
    let text = String::from_utf8_lossy(&html);
    let lower = text.to_lowercase();
    let insert_at = ["<head", "<html"].iter().find_map(|tag| {
        let start = lower.find(tag)?;
        let close = lower[start..].find('>')?;
        Some(start + close + 1)
    });
    match insert_at {
        Some(i) => {
            let mut out = String::with_capacity(text.len() + CSP_REPORTER.len());
            out.push_str(&text[..i]);
            out.push_str(CSP_REPORTER);
            out.push_str(&text[i..]);
            out.into_bytes()
        }
        None => {
            let mut out = CSP_REPORTER.as_bytes().to_vec();
            out.extend_from_slice(&html);
            out
        }
    }
}

/// harbly-asset://localhost/current/<asset_id>[?allow=<token>]
/// harbly-asset://localhost/version/<asset_id>/<ver>
/// harbly-asset://localhost/rel/<asset_id>/<path…>   (sibling images for rendered Markdown)
pub fn asset_protocol<R: Runtime>(
    ctx: UriSchemeContext<'_, R>,
    req: http::Request<Vec<u8>>,
) -> http::Response<Cow<'static, [u8]>> {
    let state = ctx.app_handle().state::<AppState>();
    let lib = match state.lib() {
        Ok(l) => l,
        Err(_) => return not_found(),
    };
    let path = req.uri().path().trim_start_matches('/');
    let segs: Vec<String> = path
        .split('/')
        .map(|s| percent_decode_str(s).decode_utf8_lossy().to_string())
        .collect();

    // Sibling-file route: images referenced (relatively) by a rendered Markdown file
    if let [kind, id, rest @ ..] = segs.as_slice() {
        if kind == "rel" {
            return serve_relative(&lib, id, rest);
        }
    }

    let (file, asset_id) = match segs.as_slice() {
        [kind, id] if kind == "current" => (lib.asset_abs_path(id).ok(), Some(id.clone())),
        [kind, id, ver] if kind == "version" => (
            ver.parse::<i64>()
                .ok()
                .map(|v| lib.version_file_path(id, v)),
            Some(id.clone()),
        ),
        _ => (None, None),
    };
    let Some(file) = file else { return not_found() };
    let Some(asset_id) = asset_id else {
        return not_found();
    };

    // Markdown is rendered server-side to a self-contained HTML document (used by
    // the thumbnail pipeline); relative images point back at the rel/ route.
    let ext = file
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if matches!(ext.as_str(), "md" | "markdown") {
        let Ok(text) = std::fs::read_to_string(&file) else {
            return not_found();
        };
        let base = format!("harbly-asset://localhost/rel/{asset_id}/");
        let body = harbly_core::md_to_html_body(&text, Some(&base));
        let lang = state.lang.lock().unwrap().clone();
        let page = crate::md_template::render_page(&body, &lang);
        return resp(
            200,
            "text/html; charset=utf-8",
            Some(MD_CSP),
            page.into_bytes(),
        );
    }

    // hdoc pages: wrap the raw source with the component runtime. Like Markdown,
    // the document itself must never execute scripts — the CSP honors only the
    // per-response nonce carried by the injected runtime, so any <script> inside
    // the (possibly AI-written) source is inert.
    if ext == "hdoc" {
        let Ok(text) = std::fs::read_to_string(&file) else {
            return not_found();
        };
        let lang = state.lang.lock().unwrap().clone();
        let t = crate::i18n::l(&lang);
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        let base = format!("harbly-asset://localhost/rel/{asset_id}/");
        let page = crate::hdoc_template::render_page(
            &text,
            &crate::hdoc_template::HdocRender {
                lang: &lang,
                toc_label: t.toc,
                rel_base: Some(&base),
                nonce: Some(&nonce),
            },
        );
        let csp = hdoc_csp(&nonce);
        return resp(
            200,
            "text/html; charset=utf-8",
            Some(&csp),
            page.into_bytes(),
        );
    }

    // One-shot allow-token check (single-use + 15s expiry + bound to the asset)
    let mut csp = SANDBOX_CSP;
    if let Some(query) = req.uri().query() {
        let token = query.split('&').find_map(|kv| kv.strip_prefix("allow="));
        if let Some(token) = token {
            let mut map = state.allow_tokens.lock().unwrap();
            if let Some((tid, at)) = map.remove(token) {
                if tid == asset_id && at.elapsed() < Duration::from_secs(15) {
                    csp = RELAXED_CSP;
                }
            }
        }
    }

    match std::fs::read(&file) {
        Ok(bytes) => resp(
            200,
            "text/html; charset=utf-8",
            Some(csp),
            inject_reporter(bytes),
        ),
        Err(_) => not_found(),
    }
}

/// Common raster/vector/media types that a Markdown document may embed. SVG is
/// safe to serve here: it is loaded via <img>, which cannot execute its scripts.
fn rel_mime(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        _ => return None,
    })
}

/// Serve a file referenced relatively by a rendered Markdown document, resolved
/// against the asset's own folder. Traversal is blocked by explicit `..`
/// rejection plus canonicalized containment under the library root — this fully
/// resolves symlinks, so any pre-existing symlink pointing outside the library is
/// rejected. The response is limited to a whitelist of embeddable media types and
/// to regular files. (The library is a single-user local trust domain: a live
/// same-user attacker swapping a path component mid-request holds no privilege the
/// app process lacks, so that residual TOCTOU is out of scope.)
fn serve_relative(
    lib: &harbly_core::Library,
    id: &str,
    rest: &[String],
) -> http::Response<Cow<'static, [u8]>> {
    if rest.is_empty() || rest.iter().any(|s| s == ".." || s.contains('\\')) {
        return not_found();
    }
    let Ok(asset_path) = lib.asset_abs_path(id) else {
        return not_found();
    };
    let Some(base_dir) = asset_path.parent() else {
        return not_found();
    };
    let target = base_dir.join(rest.join("/"));
    let (Ok(canon), Ok(root)) = (target.canonicalize(), lib.root().canonicalize()) else {
        return not_found();
    };
    // Contained under the library root, and a regular file (not a dir/fifo/device)
    if !canon.starts_with(&root) || !canon.is_file() {
        return not_found();
    }
    let mime = canon
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
        .and_then(rel_mime);
    let Some(mime) = mime else { return not_found() };
    match std::fs::read(&canon) {
        Ok(bytes) => http::Response::builder()
            .status(200)
            .header("Content-Type", mime)
            .header("Access-Control-Allow-Origin", "*")
            .header("Cache-Control", "no-store")
            .body(Cow::Owned(bytes))
            .unwrap(),
        Err(_) => not_found(),
    }
}

/// harbly-thumb://localhost/<hash>.jpg
pub fn thumb_protocol<R: Runtime>(
    ctx: UriSchemeContext<'_, R>,
    req: http::Request<Vec<u8>>,
) -> http::Response<Cow<'static, [u8]>> {
    let state = ctx.app_handle().state::<AppState>();
    let lib = match state.lib() {
        Ok(l) => l,
        Err(_) => return not_found(),
    };
    let name = req.uri().path().trim_start_matches('/');
    let hash = name.trim_end_matches(".jpg");
    if hash.is_empty() || !hash.chars().all(|c| c.is_ascii_alphanumeric()) {
        return not_found();
    }
    match std::fs::read(lib.thumb_path(hash)) {
        Ok(bytes) => resp(200, "image/jpeg", None, bytes),
        Err(_) => not_found(),
    }
}
