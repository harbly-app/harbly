use crate::state::{AppState, FileOp, OpEntry, ThumbJob};
use harbly_core::Library;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

// ---------- Config: library location & UI language (stored in the app config dir, never inside the library) ----------

fn config_path(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_config_dir().ok()?;
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join("config.json"))
}

fn read_config(app: &AppHandle) -> serde_json::Value {
    config_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

pub(crate) fn write_config_key(app: &AppHandle, key: &str, value: serde_json::Value) {
    if let Some(p) = config_path(app) {
        let mut v = read_config(app);
        if !v.is_object() {
            v = serde_json::json!({});
        }
        v[key] = value;
        let _ = std::fs::write(p, v.to_string());
    }
}

/// One top-level config.json value (used by the AI settings section).
pub(crate) fn read_config_value(app: &AppHandle, key: &str) -> Option<serde_json::Value> {
    read_config(app).get(key).cloned()
}

fn saved_library(app: &AppHandle) -> Option<PathBuf> {
    let v = read_config(app);
    let path = PathBuf::from(v.get("library")?.as_str()?);
    path.is_dir().then_some(path)
}

fn save_library(app: &AppHandle, root: &std::path::Path) {
    write_config_key(app, "library", serde_json::json!(root.to_string_lossy()));
}

pub fn saved_lang(app: &AppHandle) -> String {
    crate::i18n::normalize(
        read_config(app)
            .get("lang")
            .and_then(|v| v.as_str())
            .unwrap_or("zh-CN"),
    )
    .to_string()
}

pub(crate) fn cur_lang(app: &AppHandle) -> String {
    let l = app.state::<AppState>().lang.lock().unwrap().clone();
    crate::i18n::normalize(&l).to_string()
}

/// The currently effective language (loaded from config at startup): lets the
/// frontend align with it when it has no local preference
#[tauri::command]
pub fn get_language(app: AppHandle) -> String {
    cur_lang(&app)
}

/// Switch UI language: persist + rebuild the native menu on the main thread + sync the Undo menu text
#[tauri::command]
pub async fn set_language(app: AppHandle, lang: String) -> Result<(), String> {
    let lang = crate::i18n::normalize(&lang).to_string();
    *app.state::<AppState>().lang.lock().unwrap() = lang.clone();
    write_config_key(&app, "lang", serde_json::json!(lang));
    let app2 = app.clone();
    let lang2 = lang.clone();
    app.run_on_main_thread(move || {
        let _ = crate::menu::setup(&app2, &lang2);
    })
    .map_err(|e| e.to_string())?;
    sync_undo_menu(&app);
    Ok(())
}

// ---------- Library activation ----------

pub fn activate_library(app: &AppHandle, root: PathBuf) -> Result<String, String> {
    let lib = Arc::new(Library::open_or_create(&root).map_err(|e| e.to_string())?);
    let state = app.state::<AppState>();
    *state.library.write().unwrap() = Some(lib.clone());

    {
        let mut tx = state.thumb_tx.lock().unwrap();
        if tx.is_none() {
            *tx = Some(crate::thumbs::spawn_worker(app.clone()));
        }
    }

    // External changes (Finder moves / writes from other apps) → incremental scan
    // → notify the frontend to refresh. The app's own operations trigger this too;
    // if the scan finds no diff, no event is emitted (self-echo suppression).
    {
        let app2 = app.clone();
        let lib2 = lib.clone();
        let watcher = harbly_core::watch_library(lib.root(), move || {
            if let Ok(sum) = lib2.scan(|_| {}) {
                if sum.changed() {
                    enqueue_missing_thumbs(&app2);
                    let _ = app2.emit("library-changed", ());
                }
            }
        })
        .map_err(|e| e.to_string())?;
        *state.watcher.lock().unwrap() = Some(watcher);
    }

    // Invalidate the old library's undo log (its paths no longer belong to the current library)
    state.undo_stack.lock().unwrap().clear();
    state.redo_stack.lock().unwrap().clear();
    sync_undo_menu(app);

    save_library(app, lib.root());
    Ok(lib.root().to_string_lossy().to_string())
}

pub fn try_autoload(app: AppHandle) {
    std::thread::spawn(move || {
        if let Some(saved) = saved_library(&app) {
            let _ = activate_library(&app, saved);
        }
    });
}

pub fn enqueue_missing_thumbs(app: &AppHandle) {
    let state = app.state::<AppState>();
    let Ok(lib) = state.lib() else { return };
    let Ok(assets) = lib.all_assets() else { return };
    let tx = state.thumb_tx.lock().unwrap();
    let Some(tx) = tx.as_ref() else { return };
    for a in assets {
        if !lib.thumb_path(&a.current_hash).exists() {
            let _ = tx.send(ThumbJob {
                url: format!("harbly-asset://localhost/current/{}", a.id),
                asset_id: a.id,
                hash: a.current_hash,
            });
        }
    }
}

// ---------- Commands ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStatus {
    pub root: Option<String>,
}

#[tauri::command]
pub async fn library_status(app: AppHandle) -> LibraryStatus {
    {
        let state = app.state::<AppState>();
        let cur = state.library.read().unwrap().clone();
        if let Some(lib) = cur {
            return LibraryStatus {
                root: Some(lib.root().to_string_lossy().to_string()),
            };
        }
    }
    if let Some(saved) = saved_library(&app) {
        if let Ok(root) = activate_library(&app, saved) {
            return LibraryStatus { root: Some(root) };
        }
    }
    LibraryStatus { root: None }
}

#[tauri::command]
pub fn default_library_path(app: AppHandle) -> String {
    app.path()
        .home_dir()
        .map(|h| h.join("Harbly").to_string_lossy().to_string())
        .unwrap_or_else(|_| "~/Harbly".to_string())
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Option<String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        app2.dialog()
            .file()
            .blocking_pick_folder()
            .and_then(|f| f.into_path().ok())
            .map(|p| p.to_string_lossy().to_string())
    })
    .await
    .ok()
    .flatten()
}

#[tauri::command]
pub async fn library_init(app: AppHandle, path: String) -> Result<String, String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || activate_library(&app2, PathBuf::from(path)))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn scan_library(app: AppHandle) -> Result<harbly_core::ScanSummary, String> {
    let lib = app.state::<AppState>().lib()?;
    let app2 = app.clone();
    let sum = tauri::async_runtime::spawn_blocking(move || {
        lib.scan(move |p| {
            let _ = app2.emit("scan-progress", p);
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    enqueue_missing_thumbs(&app);
    let _ = app.emit("library-changed", ());
    Ok(sum)
}

#[tauri::command]
pub async fn rescan(app: AppHandle) -> Result<harbly_core::ScanSummary, String> {
    scan_library(app).await
}

#[tauri::command]
pub async fn dir_tree(app: AppHandle) -> Result<harbly_core::TreeNode, String> {
    app.state::<AppState>()
        .lib()?
        .dir_tree()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_assets(
    app: AppHandle,
    folder: String,
    sort: harbly_core::SortKey,
) -> Result<Vec<harbly_core::AssetMeta>, String> {
    app.state::<AppState>()
        .lib()?
        .list_assets(&folder, sort)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn asset_get(app: AppHandle, id: String) -> Result<harbly_core::AssetMeta, String> {
    app.state::<AppState>()
        .lib()?
        .asset(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn inbox_count(app: AppHandle) -> Result<i64, String> {
    app.state::<AppState>()
        .lib()?
        .inbox_count()
        .map_err(|e| e.to_string())
}

// ---------- Markdown editing ----------

/// Read an asset's current file contents (for the in-app Markdown editor)
#[tauri::command]
pub async fn asset_read_text(app: AppHandle, id: String) -> Result<String, String> {
    let lib = app.state::<AppState>().lib()?;
    tauri::async_runtime::spawn_blocking(move || lib.read_asset_text(&id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Autosave: write new text and re-index, WITHOUT versioning or notifying the UI.
/// Per-session versioning happens once, at `asset_checkpoint`. Emitting
/// library-changed here would refresh the grid mid-edit and snapshot a thumbnail
/// on every keystroke-pause, so it is deliberately omitted.
#[tauri::command]
pub async fn asset_write(
    app: AppHandle,
    id: String,
    content: String,
) -> Result<harbly_core::AssetMeta, String> {
    let lib = app.state::<AppState>().lib()?;
    tauri::async_runtime::spawn_blocking(move || lib.write_asset_text(&id, &content))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// End of an editing session: append one "编辑" version if the content changed
/// since `base_hash`. Only then does the grid refresh and the thumbnail rebuild.
#[tauri::command]
pub async fn asset_checkpoint(
    app: AppHandle,
    id: String,
    base_hash: String,
) -> Result<bool, String> {
    let lib = app.state::<AppState>().lib()?;
    let created =
        tauri::async_runtime::spawn_blocking(move || lib.checkpoint_version(&id, &base_hash))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
    if created.is_some() {
        enqueue_missing_thumbs(&app);
        let _ = app.emit("library-changed", ());
    }
    Ok(created.is_some())
}

/// Create a new empty Markdown file and open it (undoable, like New Folder)
#[tauri::command]
pub async fn asset_new_markdown(
    app: AppHandle,
    folder: String,
    name: Option<String>,
) -> Result<harbly_core::AssetMeta, String> {
    let lib = app.state::<AppState>().lib()?;
    let t = crate::i18n::l(&cur_lang(&app));
    let stem = name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| t.untitled.to_string());
    let lib2 = lib.clone();
    let a =
        tauri::async_runtime::spawn_blocking(move || lib2.create_markdown_asset(&folder, &stem))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
    record_op(
        &app,
        FileOp::Created {
            paths: vec![lib.abs(&a.rel_path)],
        },
        crate::i18n::tpl(t.op_new_md, &a.file_name),
    );
    enqueue_missing_thumbs(&app);
    let _ = app.emit("library-changed", ());
    Ok(a)
}

#[tauri::command]
pub async fn import_paths(
    app: AppHandle,
    paths: Vec<String>,
    dest: String,
) -> Result<harbly_core::ImportResult, String> {
    let lib = app.state::<AppState>().lib()?;
    let lib2 = lib.clone();
    let pbs: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let res = tauri::async_runtime::spawn_blocking(move || lib2.import_files(&pbs, &dest))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    if !res.imported.is_empty() {
        let t = crate::i18n::l(&cur_lang(&app));
        let label = if res.imported.len() == 1 {
            crate::i18n::tpl(
                t.op_import_one,
                res.imported[0]
                    .rsplit('/')
                    .next()
                    .unwrap_or(&res.imported[0]),
            )
        } else {
            crate::i18n::tpl(t.op_import_n, &res.imported.len().to_string())
        };
        record_op(
            &app,
            FileOp::Created {
                paths: res.imported.iter().map(|r| lib.abs(r)).collect(),
            },
            label,
        );
    }
    enqueue_missing_thumbs(&app);
    let _ = app.emit("library-changed", ());
    Ok(res)
}

#[tauri::command]
pub async fn pick_and_import(
    app: AppHandle,
    dest: String,
) -> Result<harbly_core::ImportResult, String> {
    let app2 = app.clone();
    let files: Vec<String> = tauri::async_runtime::spawn_blocking(move || {
        app2.dialog()
            .file()
            .add_filter("HTML / Markdown", &["html", "htm", "md", "markdown"])
            .blocking_pick_files()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|f| f.into_path().ok())
            .map(|p| p.to_string_lossy().to_string())
            .collect()
    })
    .await
    .map_err(|e| e.to_string())?;
    if files.is_empty() {
        return Ok(harbly_core::ImportResult::default());
    }
    import_paths(app, files, dest).await
}

#[tauri::command]
pub async fn search_assets(
    app: AppHandle,
    q: String,
) -> Result<Vec<harbly_core::SearchHit>, String> {
    app.state::<AppState>()
        .lib()?
        .search(&q)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn asset_rename(
    app: AppHandle,
    id: String,
    new_name: String,
) -> Result<harbly_core::AssetMeta, String> {
    let lib = app.state::<AppState>().lib()?;
    let cur = lib.asset(&id).map_err(|e| e.to_string())?;
    let old_abs = lib.abs(&cur.rel_path);
    let r = lib
        .rename_asset(&id, &new_name)
        .map_err(|e| e.to_string())?;
    if r.rel_path != cur.rel_path {
        let t = crate::i18n::l(&cur_lang(&app));
        record_op(
            &app,
            FileOp::Moved {
                moves: vec![(old_abs, lib.abs(&r.rel_path))],
            },
            crate::i18n::tpl(t.op_rename_one, &cur.file_name),
        );
    }
    let _ = app.emit("library-changed", ());
    Ok(r)
}

/// Move (a single item, or a batch as one undo group)
#[tauri::command]
pub async fn assets_move(app: AppHandle, ids: Vec<String>, dest: String) -> Result<usize, String> {
    let lib = app.state::<AppState>().lib()?;
    let app2 = app.clone();
    let n = tauri::async_runtime::spawn_blocking(move || -> Result<usize, String> {
        let mut moves: Vec<(PathBuf, PathBuf)> = vec![];
        let mut first_name = String::new();
        for id in &ids {
            let Ok(cur) = lib.asset(id) else { continue };
            if cur.folder == dest {
                continue;
            }
            let old_abs = lib.abs(&cur.rel_path);
            let r = lib.move_asset(id, &dest).map_err(|e| e.to_string())?;
            if first_name.is_empty() {
                first_name = cur.file_name;
            }
            moves.push((old_abs, lib.abs(&r.rel_path)));
        }
        let n = moves.len();
        if n > 0 {
            let t = crate::i18n::l(&cur_lang(&app2));
            let label = if n == 1 {
                crate::i18n::tpl(t.op_move_one, &first_name)
            } else {
                crate::i18n::tpl(t.op_move_n, &n.to_string())
            };
            record_op(&app2, FileOp::Moved { moves }, label);
        }
        Ok(n)
    })
    .await
    .map_err(|e| e.to_string())??;
    let _ = app.emit("library-changed", ());
    Ok(n)
}

// ---------- File-operation log: generic undo/redo with Finder semantics ----------

const UNDO_CAP: usize = 50;

/// Record a completed operation: push onto the undo stack, clear the redo stack, sync the menu text
pub(crate) fn record_op(app: &AppHandle, op: FileOp, label: impl Into<String>) {
    let state = app.state::<AppState>();
    {
        let mut s = state.undo_stack.lock().unwrap();
        s.push(OpEntry {
            op,
            label: label.into(),
        });
        if s.len() > UNDO_CAP {
            let overflow = s.len() - UNDO_CAP;
            s.drain(0..overflow);
        }
    }
    state.redo_stack.lock().unwrap().clear();
    sync_undo_menu(app);
}

/// The menu bar's Undo/Redo items show the operation about to be undone in real
/// time (same as Finder), falling back to the plain label when the stack is empty.
/// The items are always enabled — when an input field is focused, the frontend
/// routes ⌘Z to text undo instead.
fn sync_undo_menu(app: &AppHandle) {
    let state = app.state::<AppState>();
    let u = state
        .undo_stack
        .lock()
        .unwrap()
        .last()
        .map(|e| e.label.clone());
    let r = state
        .redo_stack
        .lock()
        .unwrap()
        .last()
        .map(|e| e.label.clone());
    let t = crate::i18n::l(&cur_lang(app));
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(menu) = app2.menu() else { return };
        let Ok(items) = menu.items() else { return };
        for kind in items {
            let Some(sub) = kind.as_submenu() else {
                continue;
            };
            if let Some(item) = sub.get("undo").and_then(|k| k.as_menuitem().cloned()) {
                let _ = item.set_text(
                    u.as_ref()
                        .map(|l| crate::i18n::tpl(t.undo_pat, l))
                        .unwrap_or_else(|| t.undo.to_string()),
                );
            }
            if let Some(item) = sub.get("redo").and_then(|k| k.as_menuitem().cloned()) {
                let _ = item.set_text(
                    r.as_ref()
                        .map(|l| crate::i18n::tpl(t.redo_pat, l))
                        .unwrap_or_else(|| t.redo.to_string()),
                );
            }
        }
    });
}

/// Move a path back to the target; if occupied, dodge with a suffix, and recreate
/// the parent directory if missing. Returns the actual landing path.
fn move_back(lib: &Library, from: &Path, to: &Path) -> Option<PathBuf> {
    if !from.exists() {
        return None; // Source is gone (Trash was emptied / moved again externally)
    }
    let mut dest = to.to_path_buf();
    if dest.exists() {
        let rel = dest
            .strip_prefix(lib.root())
            .ok()?
            .to_str()?
            .replace('\\', "/");
        dest = lib.abs(&lib.unique_rel(&rel));
    }
    if let Some(parent) = dest.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::rename(from, &dest).ok()?;
    Some(dest)
}

/// Execute a log entry's inverse operation (shared by undo and redo), returning
/// (the inverse entry for the opposite stack, the number actually processed).
/// Trashed→move back (inverse=Created); Moved→move back (inverse=directions swapped);
/// Created→into the Trash (inverse=Trashed).
fn execute_entry(lib: &Library, entry: OpEntry) -> (Option<OpEntry>, usize) {
    let label = entry.label;
    match entry.op {
        FileOp::Trashed { moves } => {
            let mut created = vec![];
            for (landing, orig) in &moves {
                if let Some(d) = move_back(lib, landing, orig) {
                    created.push(d);
                }
            }
            let n = created.len();
            (
                (n > 0).then_some(OpEntry {
                    op: FileOp::Created { paths: created },
                    label,
                }),
                n,
            )
        }
        FileOp::Moved { moves } => {
            let mut inv = vec![];
            for (before, after) in &moves {
                if let Some(d) = move_back(lib, after, before) {
                    // Inverse-entry semantics: was moved from after to d — redo moves d back to after
                    inv.push((after.clone(), d));
                }
            }
            let n = inv.len();
            (
                (n > 0).then_some(OpEntry {
                    op: FileOp::Moved { moves: inv },
                    label,
                }),
                n,
            )
        }
        FileOp::Created { paths } => {
            let mut trashed = vec![];
            for p in &paths {
                if !p.exists() {
                    continue;
                }
                if let Ok(landing) = crate::trash_util::trash_with_result(p) {
                    trashed.push((landing, p.clone()));
                }
            }
            let n = trashed.len();
            (
                (n > 0).then_some(OpEntry {
                    op: FileOp::Trashed { moves: trashed },
                    label,
                }),
                n,
            )
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoResult {
    pub label: String,
    pub count: usize,
}

async fn shift_stack(app: AppHandle, redo: bool) -> Result<Option<UndoResult>, String> {
    let entry = {
        let state = app.state::<AppState>();
        let popped = if redo {
            state.redo_stack.lock().unwrap().pop()
        } else {
            state.undo_stack.lock().unwrap().pop()
        };
        popped
    };
    let Some(entry) = entry else { return Ok(None) };
    let label = entry.label.clone();
    let lib = app.state::<AppState>().lib()?;
    let (inverse, n) = tauri::async_runtime::spawn_blocking(move || {
        let r = execute_entry(&lib, entry);
        let _ = lib.scan(|_| {});
        r
    })
    .await
    .map_err(|e| e.to_string())?;
    if let Some(inv) = inverse {
        let state = app.state::<AppState>();
        let target = if redo {
            &state.undo_stack
        } else {
            &state.redo_stack
        };
        target.lock().unwrap().push(inv);
    }
    sync_undo_menu(&app);
    enqueue_missing_thumbs(&app);
    let _ = app.emit("library-changed", ());
    Ok(Some(UndoResult { label, count: n }))
}

/// ⌘Z: undo the most recent file operation (delete/move/rename/duplicate/new/import/paste)
#[tauri::command]
pub async fn undo_op(app: AppHandle) -> Result<Option<UndoResult>, String> {
    shift_stack(app, false).await
}

/// ⌘⇧Z: redo
#[tauri::command]
pub async fn redo_op(app: AppHandle) -> Result<Option<UndoResult>, String> {
    shift_stack(app, true).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashResult {
    pub count: usize,
    pub undoable: bool,
}

/// Delete (a single item, or a batch as one undo group): files and version
/// directories are moved to the Trash via NSFileManager with their landing paths
/// recorded; ⌘Z = move back along the same path (same mechanism as Finder).
#[tauri::command]
pub async fn assets_trash(app: AppHandle, ids: Vec<String>) -> Result<TrashResult, String> {
    let lib = app.state::<AppState>().lib()?;
    let app2 = app.clone();
    let res = tauri::async_runtime::spawn_blocking(move || -> Result<TrashResult, String> {
        let mut moves: Vec<(PathBuf, PathBuf)> = vec![];
        let mut names: Vec<String> = vec![];
        let mut count = 0usize;
        for id in &ids {
            let Ok(a) = lib.asset(id) else { continue };
            let abs = lib.abs(&a.rel_path);
            match crate::trash_util::trash_with_result(&abs) {
                Ok(landing) => {
                    moves.push((landing, abs));
                    // The version-history directory goes to the Trash along with the
                    // file and is moved back on undo too (history is never lost)
                    let vdir = lib.versions_dir().join(id);
                    if vdir.exists() {
                        if let Ok(vt) = crate::trash_util::trash_with_result(&vdir) {
                            moves.push((vt, vdir));
                        }
                    }
                    let _ = lib.forget_asset(id);
                    names.push(a.file_name);
                    count += 1;
                }
                Err(_) => {
                    // Platform fallback: regular, non-undoable deletion
                    lib.trash_asset(id).map_err(|e| e.to_string())?;
                    count += 1;
                }
            }
        }
        let undoable = !moves.is_empty();
        if undoable {
            let t = crate::i18n::l(&cur_lang(&app2));
            let label = if names.len() == 1 {
                crate::i18n::tpl(t.op_delete_one, &names[0])
            } else {
                crate::i18n::tpl(t.op_delete_n, &names.len().to_string())
            };
            record_op(&app2, FileOp::Trashed { moves }, label);
        }
        Ok(TrashResult { count, undoable })
    })
    .await
    .map_err(|e| e.to_string())??;
    let _ = app.emit("library-changed", ());
    Ok(res)
}

#[tauri::command]
pub async fn reveal_asset(app: AppHandle, id: String) -> Result<(), String> {
    let p = app
        .state::<AppState>()
        .lib()?
        .asset_abs_path(&id)
        .map_err(|e| e.to_string())?;
    open_with_system(&p, true)
}

#[tauri::command]
pub async fn open_in_browser(app: AppHandle, id: String) -> Result<(), String> {
    let p = app
        .state::<AppState>()
        .lib()?
        .asset_abs_path(&id)
        .map_err(|e| e.to_string())?;
    open_with_system(&p, false)
}

#[tauri::command]
pub async fn reveal_folder(app: AppHandle, rel: String) -> Result<(), String> {
    let lib = app.state::<AppState>().lib()?;
    let p = if rel.is_empty() {
        lib.root().to_path_buf()
    } else {
        lib.abs(&rel)
    };
    open_with_system(&p, false)
}

#[tauri::command]
pub async fn create_folder(app: AppHandle, parent: String, name: String) -> Result<String, String> {
    let lib = app.state::<AppState>().lib()?;
    let rel = lib
        .create_folder(&parent, &name)
        .map_err(|e| e.to_string())?;
    let short = rel.rsplit('/').next().unwrap_or(&rel).to_string();
    let t = crate::i18n::l(&cur_lang(&app));
    record_op(
        &app,
        FileOp::Created {
            paths: vec![lib.abs(&rel)],
        },
        crate::i18n::tpl(t.op_new_folder, &short),
    );
    let _ = app.emit("library-changed", ());
    Ok(rel)
}

#[tauri::command]
pub async fn folder_rename(
    app: AppHandle,
    rel: String,
    new_name: String,
) -> Result<String, String> {
    let lib = app.state::<AppState>().lib()?;
    let old_abs = lib.abs(&rel);
    let old_name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
    let lib2 = lib.clone();
    let rel2 = rel.clone();
    let r = tauri::async_runtime::spawn_blocking(move || lib2.rename_folder(&rel2, &new_name))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    if r != rel {
        let t = crate::i18n::l(&cur_lang(&app));
        record_op(
            &app,
            FileOp::Moved {
                moves: vec![(old_abs, lib.abs(&r))],
            },
            crate::i18n::tpl(t.op_rename_folder, &old_name),
        );
    }
    let _ = app.emit("library-changed", ());
    Ok(r)
}

/// Delete a folder: the whole folder goes to the Trash with its landing path
/// recorded, and ⌘Z moves it back wholesale (including empty subdirectories —
/// the structure is restored exactly; tags travel with the files as xattrs, so
/// no backfill is needed)
#[tauri::command]
pub async fn folder_delete(app: AppHandle, rel: String) -> Result<bool, String> {
    if rel.is_empty() || rel == "_inbox" || rel.split('/').any(|c| c == "..") {
        return Err("不能删除此目录".to_string());
    }
    let lib = app.state::<AppState>().lib()?;
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<bool, String> {
        let abs = lib.abs(&rel);
        if !abs.is_dir() {
            return Err("文件夹不存在".to_string());
        }
        let name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
        match crate::trash_util::trash_with_result(&abs) {
            Ok(trashed) => {
                lib.scan(|_| {}).map_err(|e| e.to_string())?;
                let t = crate::i18n::l(&cur_lang(&app2));
                record_op(
                    &app2,
                    FileOp::Trashed {
                        moves: vec![(trashed, abs)],
                    },
                    crate::i18n::tpl(t.op_delete_folder, &name),
                );
                Ok(true)
            }
            Err(_) => {
                lib.delete_folder(&rel).map_err(|e| e.to_string())?;
                Ok(false)
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .inspect(|_| {
        let _ = app.emit("library-changed", ());
    })
}

/// Whether a folder holds anything besides .DS_Store — the frontend asks before
/// deleting so empty folders trash silently while non-empty ones get a confirm dialog
#[tauri::command]
pub async fn folder_has_content(app: AppHandle, rel: String) -> Result<bool, String> {
    if rel.is_empty() || rel.split('/').any(|c| c == "..") {
        return Err("不能删除此目录".to_string());
    }
    let lib = app.state::<AppState>().lib()?;
    let rd = std::fs::read_dir(lib.abs(&rel)).map_err(|e| e.to_string())?;
    Ok(rd
        .filter_map(|e| e.ok())
        .any(|e| e.file_name().to_string_lossy() != ".DS_Store"))
}

#[tauri::command]
pub async fn folder_duplicate(app: AppHandle, rel: String) -> Result<String, String> {
    let lib = app.state::<AppState>().lib()?;
    let lib2 = lib.clone();
    let src_name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
    let r = tauri::async_runtime::spawn_blocking(move || lib2.duplicate_folder(&rel))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let t = crate::i18n::l(&cur_lang(&app));
    record_op(
        &app,
        FileOp::Created {
            paths: vec![lib.abs(&r)],
        },
        crate::i18n::tpl(t.op_dup_folder, &src_name),
    );
    enqueue_missing_thumbs(&app);
    let _ = app.emit("library-changed", ());
    Ok(r)
}

#[tauri::command]
pub async fn asset_duplicate(app: AppHandle, id: String) -> Result<harbly_core::AssetMeta, String> {
    let lib = app.state::<AppState>().lib()?;
    let lib2 = lib.clone();
    let r = tauri::async_runtime::spawn_blocking(move || lib2.duplicate_asset(&id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let t = crate::i18n::l(&cur_lang(&app));
    record_op(
        &app,
        FileOp::Created {
            paths: vec![lib.abs(&r.rel_path)],
        },
        crate::i18n::tpl(t.op_dup_one, &r.file_name),
    );
    enqueue_missing_thumbs(&app);
    let _ = app.emit("library-changed", ());
    Ok(r)
}

#[tauri::command]
pub async fn list_versions(
    app: AppHandle,
    id: String,
) -> Result<Vec<harbly_core::VersionInfo>, String> {
    app.state::<AppState>()
        .lib()?
        .list_versions(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn restore_version(app: AppHandle, id: String, ver: i64) -> Result<(), String> {
    app.state::<AppState>()
        .lib()?
        .restore_version(&id, ver)
        .map_err(|e| e.to_string())?;
    enqueue_missing_thumbs(&app);
    let _ = app.emit("library-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn set_tags(app: AppHandle, id: String, tags: Vec<String>) -> Result<(), String> {
    app.state::<AppState>()
        .lib()?
        .set_tags(&id, &tags)
        .map_err(|e| e.to_string())?;
    let _ = app.emit("library-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn all_tags(app: AppHandle) -> Result<Vec<harbly_core::TagInfo>, String> {
    app.state::<AppState>()
        .lib()?
        .all_tags()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn assets_by_tag(
    app: AppHandle,
    tag: String,
) -> Result<Vec<harbly_core::AssetMeta>, String> {
    app.state::<AppState>()
        .lib()?
        .assets_by_tag(&tag)
        .map_err(|e| e.to_string())
}

/// Issue a one-shot allow token for external resources (sandboxed content has no
/// IPC access, so it cannot obtain one)
#[tauri::command]
pub async fn asset_allow_once(app: AppHandle, id: String) -> Result<String, String> {
    let state = app.state::<AppState>();
    state.lib()?.asset(&id).map_err(|e| e.to_string())?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let mut map = state.allow_tokens.lock().unwrap();
    map.retain(|_, (_, at)| at.elapsed() < std::time::Duration::from_secs(60));
    map.insert(token.clone(), (id, std::time::Instant::now()));
    Ok(token)
}

/// Export a copy of an asset: system save dialog → copy the current file
#[tauri::command]
pub async fn export_asset(app: AppHandle, id: String) -> Result<Option<String>, String> {
    let lib = app.state::<AppState>().lib()?;
    let a = lib.asset(&id).map_err(|e| e.to_string())?;
    let src = lib.asset_abs_path(&id).map_err(|e| e.to_string())?;
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let dest = app2
            .dialog()
            .file()
            .set_file_name(&a.file_name)
            .blocking_save_file()
            .and_then(|f| f.into_path().ok());
        match dest {
            Some(d) => std::fs::copy(&src, &d)
                .map(|_| {
                    let _ = harbly_core::copy_tags(&src, &d); // Exported copies keep their Finder tags
                    Some(d.to_string_lossy().to_string())
                })
                .map_err(|e| e.to_string()),
            None => Ok(None),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Export a folder as a ZIP: system save dialog → recursive packing (skips .harbly)
#[tauri::command]
pub async fn export_folder(app: AppHandle, rel: String) -> Result<Option<String>, String> {
    let lib = app.state::<AppState>().lib()?;
    let name = rel.rsplit('/').next().unwrap_or("导出").to_string();
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let dest = app2
            .dialog()
            .file()
            .set_file_name(format!("{name}.zip"))
            .blocking_save_file()
            .and_then(|f| f.into_path().ok());
        match dest {
            Some(d) => lib
                .export_folder_zip(&rel, &d)
                .map(|_| Some(d.to_string_lossy().to_string()))
                .map_err(|e| e.to_string()),
            None => Ok(None),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Clear the thumbnail cache and regenerate
#[tauri::command]
pub async fn thumbs_rebuild(app: AppHandle) -> Result<(), String> {
    let lib = app.state::<AppState>().lib()?;
    let dir = lib.thumbs_dir();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }
    enqueue_missing_thumbs(&app);
    Ok(())
}

#[tauri::command]
pub async fn request_thumbs(app: AppHandle, ids: Vec<String>) -> Result<(), String> {
    let state = app.state::<AppState>();
    let lib = state.lib()?;
    let tx = state.thumb_tx.lock().unwrap();
    let Some(tx) = tx.as_ref() else { return Ok(()) };
    for id in ids {
        if let Ok(a) = lib.asset(&id) {
            if !lib.thumb_path(&a.current_hash).exists() {
                let _ = tx.send(ThumbJob {
                    url: format!("harbly-asset://localhost/current/{}", a.id),
                    asset_id: a.id,
                    hash: a.current_hash,
                });
            }
        }
    }
    Ok(())
}

// ---------- System clipboard: file copy/paste (interoperates with Finder) ----------

/// Run on the main thread and fetch the result back (NSPasteboard/NSApplication require the main thread)
fn on_main<T: Send + 'static>(
    app: &AppHandle,
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())
}

/// ⌘C: write the selected assets to the system clipboard as file URLs — ⌘V works directly in Finder
#[tauri::command]
pub async fn pasteboard_copy(app: AppHandle, ids: Vec<String>) -> Result<usize, String> {
    let lib = app.state::<AppState>().lib()?;
    let paths: Vec<PathBuf> = ids
        .iter()
        .filter_map(|id| lib.asset_abs_path(id).ok())
        .filter(|p| p.exists())
        .collect();
    if paths.is_empty() {
        return Err("没有可拷贝的文件".into());
    }
    let n = paths.len();
    on_main(&app, move || crate::pasteboard::write_file_urls(&paths))??;
    Ok(n)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasteOutcome {
    pub count: usize,
    pub moved: usize,
    pub copied: usize,
}

/// ⌘V / ⌥⌘V: paste the clipboard's files into the target folder.
/// In-library source + ⌥⌘V = a real move; everything else = copy
/// (HTML files and folders; tag xattrs travel along).
#[tauri::command]
pub async fn pasteboard_paste(
    app: AppHandle,
    dest: String,
    move_items: bool,
) -> Result<PasteOutcome, String> {
    let lib = app.state::<AppState>().lib()?;
    let srcs: Vec<PathBuf> = on_main(&app, crate::pasteboard::read_file_paths)?
        .into_iter()
        .filter(|p| p.exists())
        .collect();
    if srcs.is_empty() {
        return Err("剪贴板中没有文件".into());
    }
    let app2 = app.clone();
    let out = tauri::async_runtime::spawn_blocking(move || -> Result<PasteOutcome, String> {
        let dest_dir = if dest.is_empty() {
            lib.root().to_path_buf()
        } else {
            lib.abs(&dest)
        };
        std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
        let mut moved: Vec<(PathBuf, PathBuf)> = vec![];
        let mut created: Vec<PathBuf> = vec![];
        for src in &srcs {
            let is_dir = src.is_dir();
            let is_managed = src
                .file_name()
                .and_then(|n| n.to_str())
                .map(harbly_core::is_managed_name)
                .unwrap_or(false);
            if !is_dir && !is_managed {
                continue; // The library only manages HTML / Markdown files
            }
            if is_dir && dest_dir.starts_with(src) {
                continue; // A folder cannot be pasted into itself
            }
            let Some(name) = src.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let in_lib = src.starts_with(lib.root());
            if move_items && in_lib && src.parent() == Some(dest_dir.as_path()) {
                continue; // Moving in place = no-op
            }
            let rel = if dest.is_empty() {
                name.to_string()
            } else {
                format!("{dest}/{name}")
            };
            let target = lib.abs(&lib.unique_rel(&rel));
            if move_items && in_lib {
                if std::fs::rename(src, &target).is_ok() {
                    moved.push((src.clone(), target));
                }
            } else if is_dir {
                if harbly_core::copy_dir_recursive(src, &target).is_ok() {
                    created.push(target);
                }
            } else if std::fs::copy(src, &target).is_ok() {
                let _ = harbly_core::copy_tags(src, &target);
                created.push(target);
            }
        }
        // Record one entry per kind (a mixed-source ⌥⌘V is rare enough that
        // undoing it in two ⌘Z steps is acceptable)
        let t = crate::i18n::l(&cur_lang(&app2));
        if !moved.is_empty() {
            record_op(
                &app2,
                FileOp::Moved {
                    moves: moved.clone(),
                },
                crate::i18n::tpl(t.op_move_n, &moved.len().to_string()),
            );
        }
        if !created.is_empty() {
            record_op(
                &app2,
                FileOp::Created {
                    paths: created.clone(),
                },
                crate::i18n::tpl(t.op_paste_n, &created.len().to_string()),
            );
        }
        let _ = lib.scan(|_| {});
        Ok(PasteOutcome {
            count: moved.len() + created.len(),
            moved: moved.len(),
            copied: created.len(),
        })
    })
    .await
    .map_err(|e| e.to_string())??;
    enqueue_missing_thumbs(&app);
    let _ = app.emit("library-changed", ());
    Ok(out)
}

/// Forward copy:/paste:/selectAll: to the first responder — with custom menu
/// items taking over ⌘C/⌘V/⌘A, text editing inside input fields still goes
/// through the system responder chain (same mechanism as the predefined items)
#[tauri::command]
pub async fn forward_edit_action(app: AppHandle, action: String) -> Result<(), String> {
    on_main(&app, move || {
        crate::pasteboard::forward_responder_action(&action)
    })?
}

/// Open an external link in the default browser (used by links inside AI
/// replies — the webview itself must never navigate).
#[tauri::command]
pub async fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("仅支持 http/https 链接".to_string());
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("暂仅支持 macOS".to_string())
    }
}

fn open_with_system(path: &std::path::Path, reveal: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = std::process::Command::new("open");
        if reveal {
            cmd.arg("-R");
        }
        cmd.arg(path).spawn().map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (path, reveal);
        Err("暂仅支持 macOS".to_string())
    }
}
