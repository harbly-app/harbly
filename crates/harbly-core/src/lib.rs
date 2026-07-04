mod db;
mod error;
mod extract;
mod ops;
mod scan;
mod tags_xattr;
mod types;
mod watch;

pub use error::{HarblyError, Result};
pub use ops::copy_dir_recursive;
pub use tags_xattr::{copy_tags, read_tags as read_file_tags, write_tags as write_file_tags};
pub use types::*;
pub use watch::watch_library;

use jieba_rs::Jieba;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const HARBLY_DIR: &str = ".harbly";
pub const INBOX_DIR: &str = "_inbox";

/// A library = a plain folder on disk.
/// All app data (index, versions, etc.) lives in the hidden .harbly/ directory inside the library, never polluting user content.
pub struct Library {
    root: PathBuf,
    pub(crate) db: Mutex<Connection>,
    pub(crate) jieba: Jieba,
}

impl Library {
    /// Open or create a library: creates the directory if missing; idempotently ensures the _inbox and .harbly structure exists.
    /// "New library" and "adopt an existing folder" share this single entry point (adoption never touches existing files).
    pub fn open_or_create(root: impl Into<PathBuf>) -> Result<Self> {
        let root: PathBuf = root.into();
        std::fs::create_dir_all(&root)?;
        std::fs::create_dir_all(root.join(INBOX_DIR))?;
        std::fs::create_dir_all(root.join(HARBLY_DIR).join("versions"))?;
        std::fs::create_dir_all(root.join(HARBLY_DIR).join("thumbs"))?;
        let conn = db::open(&root.join(HARBLY_DIR).join("index.db"))?;
        Ok(Library {
            root,
            db: Mutex::new(conn),
            jieba: Jieba::new(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn thumbs_dir(&self) -> PathBuf {
        self.root.join(HARBLY_DIR).join("thumbs")
    }

    pub fn versions_dir(&self) -> PathBuf {
        self.root.join(HARBLY_DIR).join("versions")
    }

    pub fn thumb_path(&self, hash: &str) -> PathBuf {
        self.thumbs_dir().join(format!("{hash}.jpg"))
    }

    pub fn version_file_path(&self, asset_id: &str, ver: i64) -> PathBuf {
        self.versions_dir().join(asset_id).join(format!("v{ver}.html"))
    }

    /// Relative path → absolute path. Rejects path traversal.
    pub fn abs(&self, rel: &str) -> PathBuf {
        if rel.split('/').any(|c| c == "..") {
            return self.root.join("__invalid__");
        }
        self.root.join(rel)
    }

    /// jieba-segmented text joined with spaces, used on both the FTS5 unicode61 indexing and query sides
    pub(crate) fn seg(&self, text: &str) -> String {
        self.jieba.cut_for_search(text, true).join(" ")
    }
}

pub(crate) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub(crate) fn is_html(p: &Path) -> bool {
    matches!(
        p.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("html") | Some("htm")
    )
}

pub(crate) fn parent_folder(rel: &str) -> String {
    rel.rsplit_once('/').map(|(a, _)| a.to_string()).unwrap_or_default()
}

pub(crate) fn file_stem(rel: &str) -> String {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name.rsplit_once('.').map(|(a, _)| a.to_string()).unwrap_or_else(|| name.to_string())
}

pub(crate) fn is_hidden_component(name: &std::ffi::OsStr) -> bool {
    name.to_str().map(|s| s.starts_with('.')).unwrap_or(false)
}

/// On name collision, append a suffix automatically: foo.html → foo-2.html → foo-3.html …
pub(crate) fn unique_name(dir: &Path, name: &str) -> String {
    if !dir.join(name).exists() {
        return name.to_string();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (name.to_string(), String::new()),
    };
    for i in 2..10_000 {
        let cand = format!("{stem}-{i}{ext}");
        if !dir.join(&cand).exists() {
            return cand;
        }
    }
    format!("{stem}-{}{ext}", uuid::Uuid::new_v4())
}
