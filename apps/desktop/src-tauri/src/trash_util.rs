use std::path::{Path, PathBuf};

/// Move to the system Trash and return the file's landing path inside the Trash
/// (the key to Finder-style Undo). NSFileManager.trashItemAtURL matches Finder
/// behavior: same-volume move, restorable to the original location.
#[cfg(target_os = "macos")]
pub fn trash_with_result(path: &Path) -> Result<PathBuf, String> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    let s = path.to_str().ok_or("非法路径")?;
    let fm = NSFileManager::defaultManager();
    let url = NSURL::fileURLWithPath(&NSString::from_str(s));
    let mut resulting: Option<objc2::rc::Retained<NSURL>> = None;
    fm.trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting))
        .map_err(|e| e.localizedDescription().to_string())?;
    let out = resulting
        .and_then(|u| u.path())
        .map(|p| PathBuf::from(p.to_string()))
        .ok_or("废纸篓未返回落点")?;
    Ok(out)
}

#[cfg(not(target_os = "macos"))]
pub fn trash_with_result(_path: &Path) -> Result<PathBuf, String> {
    Err("仅 macOS 支持可撤销删除".to_string())
}
