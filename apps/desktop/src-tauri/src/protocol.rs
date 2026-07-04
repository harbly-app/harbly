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

const CSP_REPORTER: &str = "<script>(function(){var n=0;addEventListener('securitypolicyviolation',function(e){n++;try{parent.postMessage({__harbly:'csp',count:n,uri:String(e.blockedURI||'')},'*')}catch(_){}})})();</script>";

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

    // One-shot allow-token check (single-use + 15s expiry + bound to the asset)
    let mut csp = SANDBOX_CSP;
    if let (Some(query), Some(id)) = (req.uri().query(), asset_id) {
        let token = query.split('&').find_map(|kv| kv.strip_prefix("allow="));
        if let Some(token) = token {
            let mut map = state.allow_tokens.lock().unwrap();
            if let Some((tid, at)) = map.remove(token) {
                if tid == id && at.elapsed() < Duration::from_secs(15) {
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
