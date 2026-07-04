use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Watch the library directory: any change on a non-hidden path fires one callback after a 700ms debounce.
/// .harbly/ (thumbnail and index writes) falls under the hidden-directory rule and is ignored, avoiding self-triggered loops.
/// The returned watcher must be held by the caller; dropping it stops watching.
pub fn watch_library(
    root: &Path,
    on_change: impl Fn() + Send + 'static,
) -> notify::Result<RecommendedWatcher> {
    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })?;
    watcher.watch(root, RecursiveMode::Recursive)?;

    let root = root.to_path_buf();
    std::thread::spawn(move || loop {
        match rx.recv() {
            Ok(ev) => {
                let relevant = match &ev {
                    Ok(e) => e.paths.iter().any(|p| is_relevant(&root, p)),
                    Err(_) => false,
                };
                if !relevant {
                    continue;
                }
                // Debounce: consecutive events within a short window coalesce into a single callback
                let deadline = Instant::now() + Duration::from_millis(700);
                loop {
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        break;
                    }
                    if rx.recv_timeout(left).is_err() {
                        break;
                    }
                }
                on_change();
            }
            Err(_) => break,
        }
    });
    Ok(watcher)
}

fn is_relevant(root: &PathBuf, p: &Path) -> bool {
    let rel = match p.strip_prefix(root) {
        Ok(r) => r,
        Err(_) => return false,
    };
    !rel.components().any(|c| {
        c.as_os_str()
            .to_str()
            .map(|s| s.starts_with('.'))
            .unwrap_or(false)
    })
}
