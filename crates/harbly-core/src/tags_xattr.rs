//! Finder tag interop: tags are stored on the file itself in the com.apple.metadata:_kMDItemUserTags
//! extended attribute (binary plist string array, entries shaped like "name" or "name\ncolor-number").
//! Bidirectionally visible to Finder / Spotlight; tags travel with the file when it is moved, copied, or backed up.
//! The asset_tags database table is demoted to a query cache, reconciled against disk (the source of truth) on scan.

use std::path::Path;

pub const XATTR_KEY: &str = "com.apple.metadata:_kMDItemUserTags";

/// Harbly's star flag, stored on the file itself like Finder tags so it
/// travels with moves/copies/backups. Attribute present = favorited.
pub const FAV_XATTR_KEY: &str = "com.harbly.favorite";

#[cfg(target_os = "macos")]
pub fn read_favorite(path: &Path) -> bool {
    matches!(xattr::get(path, FAV_XATTR_KEY), Ok(Some(_)))
}

#[cfg(target_os = "macos")]
pub fn write_favorite(path: &Path, favorite: bool) -> std::io::Result<()> {
    if favorite {
        xattr::set(path, FAV_XATTR_KEY, b"1")
    } else {
        // Removing an attribute that was never set is not an error worth surfacing
        let _ = xattr::remove(path, FAV_XATTR_KEY);
        Ok(())
    }
}

/// Read the raw entries (keeping the "\ncolor-number" suffix) so rewrites can preserve Finder colors
#[cfg(target_os = "macos")]
fn read_raw(path: &Path) -> Vec<String> {
    let Ok(Some(data)) = xattr::get(path, XATTR_KEY) else {
        return vec![];
    };
    plist::from_bytes::<Vec<String>>(&data).unwrap_or_default()
}

/// The file's current tag names (color suffix stripped, deduped in order of appearance)
#[cfg(target_os = "macos")]
pub fn read_tags(path: &Path) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for raw in read_raw(path) {
        let name = raw.split('\n').next().unwrap_or("").trim().to_string();
        if !name.is_empty() && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// Overwrite the file's tags; Finder colors of existing same-name entries are preserved. Empty list = remove the attribute.
#[cfg(target_os = "macos")]
pub fn write_tags(path: &Path, tags: &[String]) -> std::io::Result<()> {
    let mut clean: Vec<&str> = vec![];
    for t in tags {
        let t = t.trim();
        if !t.is_empty() && !clean.contains(&t) {
            clean.push(t);
        }
    }
    if clean.is_empty() {
        let _ = xattr::remove(path, XATTR_KEY); // an error from the attribute not existing in the first place is irrelevant
        return Ok(());
    }
    let colored = read_raw(path);
    let entries: Vec<String> = clean
        .iter()
        .map(|name| {
            colored
                .iter()
                .find(|c| c.split('\n').next().map(str::trim) == Some(*name))
                .cloned()
                .unwrap_or_else(|| (*name).to_string())
        })
        .collect();
    let mut cur = std::io::Cursor::new(Vec::new());
    plist::to_writer_binary(&mut cur, &entries)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    xattr::set(path, XATTR_KEY, &cur.into_inner())
}

/// Copy the tag + favorite attributes verbatim — std::fs::copy does not carry
/// xattrs, so patch them up after copying a file/folder
#[cfg(target_os = "macos")]
pub fn copy_tags(src: &Path, dst: &Path) -> std::io::Result<()> {
    if let Ok(Some(data)) = xattr::get(src, FAV_XATTR_KEY) {
        let _ = xattr::set(dst, FAV_XATTR_KEY, &data);
    }
    match xattr::get(src, XATTR_KEY) {
        Ok(Some(data)) => xattr::set(dst, XATTR_KEY, &data),
        _ => Ok(()),
    }
}

// Non-macOS: no-op implementations; tags and stars live only in the database
#[cfg(not(target_os = "macos"))]
pub fn read_tags(_: &Path) -> Vec<String> {
    vec![]
}
#[cfg(not(target_os = "macos"))]
pub fn write_tags(_: &Path, _: &[String]) -> std::io::Result<()> {
    Ok(())
}
#[cfg(not(target_os = "macos"))]
pub fn copy_tags(_: &Path, _: &Path) -> std::io::Result<()> {
    Ok(())
}
#[cfg(not(target_os = "macos"))]
pub fn read_favorite(_: &Path) -> bool {
    false
}
#[cfg(not(target_os = "macos"))]
pub fn write_favorite(_: &Path, _: bool) -> std::io::Result<()> {
    Ok(())
}
