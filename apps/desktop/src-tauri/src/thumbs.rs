use crate::state::ThumbJob;
use std::sync::mpsc::{Receiver, Sender};
use tauri::{AppHandle, Emitter, Manager};

const THUMB_WINDOW: &str = "thumb-worker";

/// Spawn the thumbnail worker thread: consumes the job queue serially.
/// Implementation: a hidden WebviewWindow loads the sandboxed harbly-asset://
/// content, and once loading finishes we capture a snapshot of the web content
/// via the platform-native API (macOS: WKWebView takeSnapshot).
pub fn spawn_worker(app: AppHandle) -> Sender<ThumbJob> {
    let (tx, rx) = std::sync::mpsc::channel::<ThumbJob>();
    std::thread::spawn(move || worker_loop(app, rx));
    tx
}

fn worker_loop(app: AppHandle, rx: Receiver<ThumbJob>) {
    // Content hashes whose render failed this session. A hidden-WebView snapshot
    // is expensive, and without this a failure is retried on every enqueue
    // (scroll/scan) forever. Kept in memory, not on disk, so a relaunch retries:
    // transient failures (WebView busy, app quit mid-render) self-heal, while a
    // genuinely unrenderable file stops churning for the rest of the session.
    let mut failed: std::collections::HashSet<String> = std::collections::HashSet::new();
    while let Ok(job) = rx.recv() {
        let state = app.state::<crate::state::AppState>();
        let Ok(lib) = state.lib() else { continue };
        let dest = lib.thumb_path(&job.hash);
        if dest.exists() || failed.contains(&job.hash) {
            continue;
        }
        match render_thumb(&app, &job.url) {
            Some(bytes) => {
                if std::fs::write(&dest, bytes).is_ok() {
                    let _ = app.emit("thumb-updated", &job);
                }
            }
            None => {
                // Render failure/timeout: remember it so we don't re-render this
                // content again this session; the card falls back to a code icon.
                failed.insert(job.hash.clone());
                let _ = app.emit("thumb-failed", &job);
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn render_thumb(app: &AppHandle, url: &str) -> Option<Vec<u8>> {
    use std::time::Duration;

    let window = ensure_window(app, url)?;

    // Wait for loading to finish (poll WKWebView.isLoading), capped at 6s
    let mut loaded = false;
    for _ in 0..60 {
        std::thread::sleep(Duration::from_millis(100));
        let (ltx, lrx) = std::sync::mpsc::channel::<bool>();
        let ok = window.with_webview(move |wv| {
            let loading = unsafe {
                let webview: &objc2_web_kit::WKWebView = &*wv.inner().cast();
                webview.isLoading()
            };
            let _ = ltx.send(!loading);
        });
        if ok.is_err() {
            return None;
        }
        if let Ok(done) = lrx.recv_timeout(Duration::from_millis(500)) {
            if done {
                loaded = true;
                break;
            }
        }
    }
    if !loaded {
        return None;
    }
    // Give script-driven rendering a settle period
    std::thread::sleep(Duration::from_millis(900));

    let (ttx, trx) = std::sync::mpsc::channel::<Option<Vec<u8>>>();
    let ok = window.with_webview(move |wv| unsafe {
        use block2::RcBlock;
        use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
        use objc2_foundation::{NSDictionary, NSError, NSNumber};
        use objc2_web_kit::{WKSnapshotConfiguration, WKWebView};

        // The with_webview closure is guaranteed to run on the main thread
        let mtm = objc2::MainThreadMarker::new_unchecked();
        let webview: &WKWebView = &*wv.inner().cast();
        let config = WKSnapshotConfiguration::new(mtm);
        // Have WebKit output a 640pt-wide scaled image directly, skipping post-processing
        config.setSnapshotWidth(Some(&NSNumber::new_f64(640.0)));

        let block = RcBlock::new(move |img: *mut NSImage, _err: *mut NSError| {
            let out: Option<Vec<u8>> = img.as_ref().and_then(|im| {
                let tiff = im.TIFFRepresentation()?;
                let rep = NSBitmapImageRep::imageRepWithData(&tiff)?;
                let props = NSDictionary::new();
                let data =
                    rep.representationUsingType_properties(NSBitmapImageFileType::JPEG, &props)?;
                Some(data.to_vec())
            });
            let _ = ttx.send(out);
        });
        webview.takeSnapshotWithConfiguration_completionHandler(Some(&config), &block);
    });
    if ok.is_err() {
        return None;
    }
    trx.recv_timeout(Duration::from_secs(8)).ok().flatten()
}

#[cfg(target_os = "macos")]
fn ensure_window(app: &AppHandle, url: &str) -> Option<tauri::WebviewWindow> {
    let parsed: tauri::Url = url.parse().ok()?;
    if let Some(w) = app.get_webview_window(THUMB_WINDOW) {
        // Reuse the hidden window; just navigate to the new URL
        if w.navigate(parsed.clone()).is_ok() {
            return Some(w);
        }
        let _ = w.close();
    }
    let w =
        tauri::WebviewWindowBuilder::new(app, THUMB_WINDOW, tauri::WebviewUrl::External(parsed))
            .visible(false)
            .focused(false)
            .inner_size(1160.0, 760.0)
            .build()
            .ok()?;
    Some(w)
}

#[cfg(not(target_os = "macos"))]
fn render_thumb(_app: &AppHandle, _url: &str) -> Option<Vec<u8>> {
    // Native snapshot on other platforms (WebView2 CapturePreview / webkitgtk snapshot) to be implemented later
    None
}
