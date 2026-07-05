mod ai_meta;
mod db;
mod error;
mod extract;
mod hdoc;
mod markdown;
mod ops;
mod scan;
mod tags_xattr;
mod types;
mod watch;

pub use ai_meta::{
    AiMessage, AiRunRecord, AiSession, AiSessionSnapshot, AiToolCtx, AiWriteOutcome, NewAiRun,
    AI_VERSION_LABEL,
};
pub use error::{HarblyError, Result};
pub use hdoc::{HDOC_NEW_TEMPLATE, HDOC_VOCAB_VERSION};
pub use markdown::md_to_html_body;
pub use ops::copy_dir_recursive;
pub use tags_xattr::{copy_tags, read_tags as read_file_tags, write_tags as write_file_tags};
pub use types::*;
pub use watch::watch_library;

use jieba_rs::Jieba;
use rusqlite::{Connection, OptionalExtension};
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
        let lib = Library {
            root,
            db: Mutex::new(conn),
            jieba: Jieba::new(),
        };
        // Heal crash leftovers; never fail opening a library over it
        let _ = lib.sweep_orphan_versions();
        Ok(lib)
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
        let ext = self.asset_ext(asset_id);
        self.versions_dir()
            .join(asset_id)
            .join(format!("v{ver}.{ext}"))
    }

    /// The on-disk extension used for an asset's version snapshots. It follows the
    /// asset's current file (renames never change type), defaulting to "html".
    pub(crate) fn asset_ext(&self, asset_id: &str) -> String {
        let rel: Option<String> = self
            .db
            .lock()
            .unwrap()
            .query_row("SELECT rel_path FROM assets WHERE id=?1", [asset_id], |r| {
                r.get(0)
            })
            .optional()
            .ok()
            .flatten();
        rel.as_deref()
            .and_then(ext_of)
            .unwrap_or_else(|| "html".to_string())
    }

    /// Relative path → absolute path. Rejects traversal AND absolute inputs:
    /// `Path::join` DISCARDS the base when handed an absolute path, so an
    /// absolute `rel` (e.g. model-supplied) would silently escape the root.
    pub fn abs(&self, rel: &str) -> PathBuf {
        if Path::new(rel).is_absolute() || rel.split('/').any(|c| c == "..") {
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

/// File modification time in whole seconds (0 if unavailable). Stored on the
/// asset row so a later watcher-driven scan can skip re-hashing unchanged files.
pub(crate) fn mtime_secs(md: &std::fs::Metadata) -> i64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The kinds of file the library manages, distinguished by extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AssetKind {
    Html,
    Markdown,
    /// Harbly page document: constrained custom-element HTML (see hdoc.rs).
    Hdoc,
}

/// Classify a path by extension; `None` for anything the library doesn't manage.
pub(crate) fn asset_kind(p: &Path) -> Option<AssetKind> {
    match p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("html") | Some("htm") => Some(AssetKind::Html),
        Some("md") | Some("markdown") => Some(AssetKind::Markdown),
        Some("hdoc") => Some(AssetKind::Hdoc),
        _ => None,
    }
}

/// Whether a path is a library-managed asset (HTML, Markdown or hdoc). Gates
/// the scanner and importer.
pub(crate) fn is_asset(p: &Path) -> bool {
    asset_kind(p).is_some()
}

/// The same predicate as [`is_asset`] but for a bare file name — exported for the
/// shell layer's pasteboard filter, which reasons about names rather than paths.
pub fn is_managed_name(name: &str) -> bool {
    asset_kind(Path::new(name)).is_some()
}

/// The lowercased extension of a relative path, if any.
pub(crate) fn ext_of(rel: &str) -> Option<String> {
    Path::new(rel)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
}

pub(crate) fn parent_folder(rel: &str) -> String {
    rel.rsplit_once('/')
        .map(|(a, _)| a.to_string())
        .unwrap_or_default()
}

pub(crate) fn file_stem(rel: &str) -> String {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name.rsplit_once('.')
        .map(|(a, _)| a.to_string())
        .unwrap_or_else(|| name.to_string())
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
