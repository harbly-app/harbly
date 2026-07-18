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
    /// Serializes undo/redo execution end-to-end. Executing an entry spans a
    /// spawn_blocking (file moves + a full scan, seconds on a big library);
    /// a second ⌘Z arriving meanwhile must wait for it, not run a second
    /// inverse concurrently over the same paths.
    pub undo_exec: tokio::sync::Mutex<()>,
    /// Bumped on every library activation. In-flight undo executions compare
    /// generations before pushing their inverse entry, so an entry carrying
    /// the OLD library's absolute paths can never enter the NEW library's
    /// stacks (where ⌘Z would replay it against the wrong library).
    pub lib_generation: std::sync::atomic::AtomicU64,
    /// Running AI tasks: frontend job id → cancel flag. Entries are removed
    /// when the run finishes; ai_cancel flips the flag cooperatively.
    pub ai_jobs: Mutex<std::collections::HashMap<String, harbly_ai::CancelFlag>>,
    /// Sessions with a turn in flight → that turn's cancel flag. The backend
    /// guard against concurrent turns interleaving one transcript (the UI
    /// serializes, but correctness must not depend on it); the flag lets
    /// session deletion cancel and await the in-flight turn.
    pub ai_busy: Mutex<std::collections::HashMap<String, harbly_ai::CancelFlag>>,
    /// Most recent deleted AI session, held for the undo toast (single slot).
    pub ai_deleted_session: Mutex<Option<harbly_core::AiSessionSnapshot>>,
    /// Detected agent CLIs by kind id — the per-send path must not respawn
    /// `--version` probes (hundreds of ms before every turn). Refreshed by
    /// ai_detect_agents whenever the panel or settings re-probe.
    pub agent_cache: Mutex<std::collections::HashMap<String, harbly_ai::AgentInfo>>,
}

/// A completed file operation. Executing it (undo and redo share one executor)
/// = applying its inverse:
/// Trashed → move back from the Trash to the original location; inverse result is Created;
/// Moved   → move back from after to before; inverse result is a Moved with directions swapped;
/// Created → move into the Trash; inverse result is Trashed. The three form a
/// closed loop, so undo and redo are naturally symmetric.
pub enum FileOp {
    /// Items are now in the Trash: (landing path in Trash, original location).
    /// `assets` snapshots the rows forgotten alongside the files — the trash
    /// undo re-registers each under its ORIGINAL id, reconnecting the version
    /// chain and AI-run history that a fresh scan-minted id would orphan.
    Trashed {
        moves: Vec<(PathBuf, PathBuf)>,
        assets: Vec<harbly_core::AssetMeta>,
    },
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
