use harbly_core::Library;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbJob {
    pub asset_id: String,
    pub hash: String,
    pub url: String,
}

#[derive(Default)]
pub struct AppState {
    pub library: RwLock<Option<Arc<Library>>>,
    /// Holds the watcher to keep it alive; replaced when switching libraries
    pub watcher: Mutex<Option<notify::RecommendedWatcher>>,
    pub thumb_tx: Mutex<Option<std::sync::mpsc::Sender<ThumbJob>>>,
    /// One-shot allow tokens for external resources: token → (asset_id, issued-at).
    /// Sandboxed content has no IPC access and cannot obtain a token, so an asset
    /// cannot grant itself network access.
    pub allow_tokens: Mutex<std::collections::HashMap<String, (String, std::time::Instant)>>,
    /// File-operation log (Finder semantics): ⌘Z pops the undo stack, runs the
    /// inverse operation, and pushes onto the redo stack; a new operation clears
    /// the redo stack; switching libraries clears both stacks.
    pub undo_stack: Mutex<Vec<OpEntry>>,
    pub redo_stack: Mutex<Vec<OpEntry>>,
    /// UI language (zh-CN/zh-TW/en/ja/ko/es), persisted in config.json
    pub lang: Mutex<String>,
}

/// A completed file operation. Executing it (undo and redo share one executor)
/// = applying its inverse:
/// Trashed → move back from the Trash to the original location; inverse result is Created;
/// Moved   → move back from after to before; inverse result is a Moved with directions swapped;
/// Created → move into the Trash; inverse result is Trashed. The three form a
/// closed loop, so undo and redo are naturally symmetric.
pub enum FileOp {
    /// Items are now in the Trash: (landing path in Trash, original location)
    Trashed { moves: Vec<(PathBuf, PathBuf)> },
    /// Items were moved from before to after: (before, after)
    Moved { moves: Vec<(PathBuf, PathBuf)> },
    /// Items were produced by this operation (copy/new/import/paste) and now live at these paths
    Created { paths: Vec<PathBuf> },
}

pub struct OpEntry {
    pub op: FileOp,
    /// Human-readable description, e.g. Delete "quote.html" — used by the
    /// menu bar's "Undo Delete…" item and toasts
    pub label: String,
}

impl AppState {
    pub fn lib(&self) -> Result<Arc<Library>, String> {
        self.library
            .read()
            .unwrap()
            .clone()
            .ok_or_else(|| "尚未选择库".to_string())
    }
}
