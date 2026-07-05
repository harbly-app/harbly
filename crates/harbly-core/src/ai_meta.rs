//! AI conversation state + the write-back paths for AI-produced content.
//!
//! Three kinds of rows, all library-scoped (they live in .harbly/index.db and
//! travel with the folder):
//! - sessions: one per conversation — supply/model/effort choice, plus the
//!   agent CLI's own session id so `--resume` can continue it;
//! - messages: the transcript (user / assistant turns, with tool-action labels);
//! - runs: the write ledger — one row per asset outcome, pointing at the
//!   version it produced and the session/message that caused it. The per-file
//!   timeline joins versions with runs; the chat renders from messages.
//!
//! The engine lives in the separate harbly-ai crate — this module is the only
//! place AI output enters the library (both the app's tool executor and the
//! MCP server write through here).

use crate::error::Result;
use crate::{now, unique_name, Library};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSession {
    pub id: String,
    /// Auto-filled from the first user message when empty.
    pub title: String,
    pub supply: String,
    pub model: String,
    /// "" | "low" | "medium" | "high" — interpreted per supply.
    pub effort: String,
    /// The agent CLI's own session id (claude --resume), when applicable.
    pub agent_session_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiMessage {
    pub id: String,
    pub session_id: String,
    /// "user" | "assistant"
    pub role: String,
    pub content: String,
    /// Tool-activity labels shown above the assistant text.
    pub actions: Vec<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiRunRecord {
    pub id: String,
    pub asset_id: String,
    /// "revise" (produced a version) | "create" (new asset) | "reply"
    pub kind: String,
    pub supply: String,
    pub model: String,
    pub instruction: String,
    /// "ok" | "error" | "cancelled"
    pub status: String,
    pub ver: Option<i64>,
    pub report: Option<String>,
    pub error: Option<String>,
    pub session_id: Option<String>,
    pub message_id: Option<String>,
    pub created_at: i64,
}

/// Everything the caller knows about a finished run; id/timestamp are minted here.
#[derive(Debug, Clone, Default)]
pub struct NewAiRun {
    pub asset_id: String,
    pub kind: String,
    pub supply: String,
    pub model: String,
    pub instruction: String,
    pub status: String,
    pub ver: Option<i64>,
    pub report: Option<String>,
    pub error: Option<String>,
    pub session_id: Option<String>,
    pub message_id: Option<String>,
}

/// Version-chain label for AI rewrites (canonical Chinese like every label
/// the core writes; the frontend localizes known labels for display).
pub const AI_VERSION_LABEL: &str = "AI 改版";

/// Attribution carried by every AI tool call: which supply/session caused a
/// write. The message id is linked after the turn completes (the assistant
/// message doesn't exist yet while tools are running).
#[derive(Debug, Clone, Default)]
pub struct AiToolCtx {
    pub supply: String,
    pub model: String,
    pub session_id: Option<String>,
}

/// What a mutating tool call did — callers use it to refresh UI / enqueue
/// thumbs. `ver` is 0 for deletions (there is no version to point at).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiWriteOutcome {
    pub asset_id: String,
    pub file_name: String,
    pub ver: i64,
    pub created: bool,
}

/// Everything needed to undo a session deletion (kept in app memory, one slot).
#[derive(Debug, Clone)]
pub struct AiSessionSnapshot {
    pub session: AiSession,
    pub messages: Vec<AiMessage>,
    /// run id → message back-link, so undo restores ledger attribution too.
    pub run_links: Vec<(String, Option<String>)>,
}

const RUN_COLS: &str =
    "id, asset_id, kind, supply, model, instruction, status, ver, report, error, session_id, message_id, created_at";

fn row_to_run(r: &rusqlite::Row) -> rusqlite::Result<AiRunRecord> {
    Ok(AiRunRecord {
        id: r.get(0)?,
        asset_id: r.get(1)?,
        kind: r.get(2)?,
        supply: r.get(3)?,
        model: r.get(4)?,
        instruction: r.get(5)?,
        status: r.get(6)?,
        ver: r.get(7)?,
        report: r.get(8)?,
        error: r.get(9)?,
        session_id: r.get(10)?,
        message_id: r.get(11)?,
        created_at: r.get(12)?,
    })
}

impl Library {
    // ---------- Write-back ----------

    /// Write AI-produced content to an existing asset and append a version in
    /// one step. No confirmation gate by design: the version chain IS the
    /// safety net (rollback-grade undo), matching the product's undo-over-
    /// confirm rule. Returns the version number the content landed as.
    pub fn apply_ai_output(&self, id: &str, text: &str, label: &str) -> Result<i64> {
        let content = text.as_bytes();
        let hash = blake3::hash(content).to_hex().to_string();
        // Snapshot BEFORE mutating the live file: if the second step fails,
        // the worst leftover is an unapplied version — never a changed file
        // without its safety-net snapshot.
        let ver = self.write_version(id, content, &hash, label)?;
        self.write_asset_text(id, text)?;
        Ok(ver)
    }

    /// Create a brand-new asset from AI output. `name` may carry a managed
    /// extension; anything else gets ".html" appended. Lands in `folder`
    /// ("" = library root), auto-suffixed on collision, source = "ai".
    pub fn create_asset_from_ai(
        &self,
        folder: &str,
        name: &str,
        content: &str,
    ) -> Result<crate::AssetMeta> {
        let mut name = name.trim().replace('/', "-");
        if name.is_empty() {
            name = "Untitled".to_string();
        }
        if !crate::is_managed_name(&name) {
            name.push_str(".html");
        }
        let dir = if folder.is_empty() {
            self.root().to_path_buf()
        } else {
            self.abs(folder)
        };
        std::fs::create_dir_all(&dir)?;
        let name = unique_name(&dir, &name);
        let abs = dir.join(&name);
        std::fs::write(&abs, content.as_bytes())?;
        let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
        let md = std::fs::metadata(&abs)?;
        let rel = if folder.is_empty() {
            name.clone()
        } else {
            format!("{folder}/{name}")
        };
        let id = self.insert_new_asset(
            &rel,
            content.as_bytes(),
            &hash,
            md.len() as i64,
            crate::mtime_secs(&md),
            "ai",
            "AI 创建",
        )?;
        self.asset(&id)
    }

    // ---------- The AI tool surface ----------

    /// Execute one library tool on behalf of a model. This single function
    /// backs BOTH consumers — the app's in-process executor (BYOK loops,
    /// codex write-back) and the harbly-mcp server (claude / external CLIs) —
    /// so every supply sees the identical world and every write lands as an
    /// attributed version. Errors are plain strings: they go back to the
    /// model, which is expected to adapt.
    pub fn execute_ai_tool(
        &self,
        name: &str,
        args: &serde_json::Value,
        ctx: &AiToolCtx,
    ) -> std::result::Result<(serde_json::Value, Option<AiWriteOutcome>), String> {
        use serde_json::json;
        let arg_str = |key: &str| -> std::result::Result<String, String> {
            args[key]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .map(String::from)
                .ok_or_else(|| format!("missing required argument: {key}"))
        };
        // Model-supplied folders must stay inside the library: reject both
        // traversal and absolute paths (Path::join would swap the root out).
        let check_folder = |folder: &str| -> std::result::Result<(), String> {
            if std::path::Path::new(folder).is_absolute() || folder.split('/').any(|c| c == "..") {
                return Err(
                    "invalid folder: must be a relative path inside the library".to_string()
                );
            }
            Ok(())
        };
        match name {
            "list_assets" => {
                let folder = args["folder"].as_str().unwrap_or("");
                check_folder(folder)?;
                // "" lists the whole library recursively (inbox excluded by
                // list_assets' own convention only for direct folders, so use
                // all_assets for the root to include everything)
                let assets = if folder.is_empty() {
                    self.all_assets().map_err(|e| e.to_string())?
                } else {
                    self.list_assets(folder, crate::SortKey::Name)
                        .map_err(|e| e.to_string())?
                };
                let total = assets.len();
                let items: Vec<_> = assets
                    .iter()
                    .take(300)
                    .map(|a| {
                        json!({
                            "asset_id": a.id,
                            "file_name": a.file_name,
                            "folder": a.folder,
                            "title": a.title,
                            "size_bytes": a.size_bytes,
                            "ver_count": a.ver_count,
                        })
                    })
                    .collect();
                Ok((json!({ "total": total, "assets": items }), None))
            }
            "delete_asset" => {
                let id = arg_str("asset_id")?;
                let a = self.asset(&id).map_err(|_| "asset not found".to_string())?;
                // System Trash, not oblivion: the user can restore from Finder.
                self.trash_asset(&id).map_err(|e| e.to_string())?;
                Ok((
                    json!({ "deleted": a.file_name, "note": "moved to system Trash (user-recoverable)" }),
                    Some(AiWriteOutcome {
                        asset_id: id,
                        file_name: a.file_name,
                        ver: 0,
                        created: false,
                    }),
                ))
            }
            "search_library" => {
                let query = arg_str("query")?;
                let hits = self.search(&query).map_err(|e| e.to_string())?;
                let results: Vec<_> = hits
                    .iter()
                    .take(20)
                    .map(|h| {
                        json!({
                            "asset_id": h.asset.id,
                            "title": h.asset.title,
                            "file_name": h.asset.file_name,
                            "folder": h.asset.folder,
                            "snippet": h.snippet,
                        })
                    })
                    .collect();
                Ok((json!({ "results": results }), None))
            }
            "read_asset" => {
                let id = arg_str("asset_id")?;
                let a = self.asset(&id).map_err(|_| "asset not found".to_string())?;
                // Gate on the indexed size BEFORE reading: a multi-hundred-MB
                // file must not be pulled into memory just to be rejected.
                // The post-read check stays for files grown since the scan.
                if a.size_bytes > 400 * 1024 {
                    return Err("file too large to read into context".to_string());
                }
                let content = self.read_asset_text(&id).map_err(|e| e.to_string())?;
                if content.len() > 400 * 1024 {
                    return Err("file too large to read into context".to_string());
                }
                Ok((
                    json!({
                        "asset_id": a.id,
                        "file_name": a.file_name,
                        "title": a.title,
                        "content": content,
                    }),
                    None,
                ))
            }
            "write_asset" => {
                let id = arg_str("asset_id")?;
                let content = arg_str("content")?;
                let summary = args["summary"].as_str().unwrap_or("").to_string();
                let a = self.asset(&id).map_err(|_| "asset not found".to_string())?;
                let ver = self
                    .apply_ai_output(&id, &content, AI_VERSION_LABEL)
                    .map_err(|e| e.to_string())?;
                let _ = self.record_ai_run(&NewAiRun {
                    asset_id: id.clone(),
                    kind: "revise".into(),
                    supply: ctx.supply.clone(),
                    model: ctx.model.clone(),
                    instruction: summary,
                    status: "ok".into(),
                    ver: Some(ver),
                    session_id: ctx.session_id.clone(),
                    ..Default::default()
                });
                Ok((
                    json!({ "ver": ver }),
                    Some(AiWriteOutcome {
                        asset_id: id,
                        file_name: a.file_name,
                        ver,
                        created: false,
                    }),
                ))
            }
            "create_asset" => {
                let name_arg = arg_str("name")?;
                let content = arg_str("content")?;
                let folder = args["folder"].as_str().unwrap_or("");
                check_folder(folder)?;
                let a = self
                    .create_asset_from_ai(folder, &name_arg, &content)
                    .map_err(|e| e.to_string())?;
                let _ = self.record_ai_run(&NewAiRun {
                    asset_id: a.id.clone(),
                    kind: "create".into(),
                    supply: ctx.supply.clone(),
                    model: ctx.model.clone(),
                    instruction: format!("create {}", a.file_name),
                    status: "ok".into(),
                    ver: Some(1),
                    session_id: ctx.session_id.clone(),
                    ..Default::default()
                });
                Ok((
                    json!({
                        "asset_id": a.id,
                        "file_name": a.file_name,
                        "folder": a.folder,
                    }),
                    Some(AiWriteOutcome {
                        asset_id: a.id,
                        file_name: a.file_name,
                        ver: 1,
                        created: true,
                    }),
                ))
            }
            _ => Err(format!("unknown tool: {name}")),
        }
    }

    /// Back-link the runs a finished turn produced to its assistant message
    /// (works across processes — MCP-server writes share the database).
    pub fn link_runs_to_message(
        &self,
        session_id: &str,
        message_id: &str,
        since: i64,
    ) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE ai_runs SET message_id=?2 WHERE session_id=?1 AND message_id IS NULL AND created_at>=?3",
            params![session_id, message_id, since],
        )?;
        Ok(())
    }

    // ---------- Sessions ----------

    pub fn create_ai_session(&self, supply: &str, model: &str, effort: &str) -> Result<AiSession> {
        let id = uuid::Uuid::new_v4().to_string();
        let t = now();
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO ai_sessions(id, title, supply, model, effort, agent_session_id, created_at, updated_at)
             VALUES(?1, '', ?2, ?3, ?4, NULL, ?5, ?5)",
            params![id, supply, model, effort, t],
        )?;
        Ok(AiSession {
            id,
            title: String::new(),
            supply: supply.to_string(),
            model: model.to_string(),
            effort: effort.to_string(),
            agent_session_id: None,
            created_at: t,
            updated_at: t,
        })
    }

    pub fn get_ai_session(&self, id: &str) -> Result<Option<AiSession>> {
        let db = self.db.lock().unwrap();
        Ok(db
            .query_row(
                "SELECT id, title, supply, model, effort, agent_session_id, created_at, updated_at
                 FROM ai_sessions WHERE id=?1",
                [id],
                |r| {
                    Ok(AiSession {
                        id: r.get(0)?,
                        title: r.get(1)?,
                        supply: r.get(2)?,
                        model: r.get(3)?,
                        effort: r.get(4)?,
                        agent_session_id: r.get(5)?,
                        created_at: r.get(6)?,
                        updated_at: r.get(7)?,
                    })
                },
            )
            .optional()?)
    }

    /// Newest activity first.
    pub fn list_ai_sessions(&self, limit: i64) -> Result<Vec<AiSession>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, title, supply, model, effort, agent_session_id, created_at, updated_at
             FROM ai_sessions ORDER BY updated_at DESC, rowid DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map([limit], |r| {
                Ok(AiSession {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    supply: r.get(2)?,
                    model: r.get(3)?,
                    effort: r.get(4)?,
                    agent_session_id: r.get(5)?,
                    created_at: r.get(6)?,
                    updated_at: r.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Change the session's supply/model/effort (the picker in the panel
    /// header). The agent session id is reset when the supply changes — a
    /// resume id from one CLI means nothing to another.
    pub fn set_ai_session_prefs(
        &self,
        id: &str,
        supply: &str,
        model: &str,
        effort: &str,
    ) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE ai_sessions SET
               agent_session_id = CASE WHEN supply=?2 THEN agent_session_id ELSE NULL END,
               supply=?2, model=?3, effort=?4, updated_at=?5
             WHERE id=?1",
            params![id, supply, model, effort, now()],
        )?;
        Ok(())
    }

    pub fn set_ai_session_agent_id(&self, id: &str, agent_session_id: &str) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE ai_sessions SET agent_session_id=?2 WHERE id=?1",
            params![id, agent_session_id],
        )?;
        Ok(())
    }

    /// Drop a dead agent resume id (the CLI garbage-collects its sessions);
    /// the next turn replays the transcript app-side instead.
    pub fn clear_ai_session_agent_id(&self, id: &str) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE ai_sessions SET agent_session_id=NULL WHERE id=?1",
            [id],
        )?;
        Ok(())
    }

    /// Delete the conversation and hand back a snapshot that
    /// [`Library::restore_ai_session`] can resurrect — panel deletion is
    /// undo-based, not confirm-based (the product's undo-over-confirm rule).
    /// Assets and versions the session produced are untouched; their run rows
    /// lose the back-link (session_id nulled) but stay on the file timeline.
    /// Returns None when the session does not exist.
    pub fn delete_ai_session(&self, id: &str) -> Result<Option<AiSessionSnapshot>> {
        let Some(session) = self.get_ai_session(id)? else {
            return Ok(None);
        };
        let messages = self.list_ai_messages(id)?;
        let db = self.db.lock().unwrap();
        let run_links: Vec<(String, Option<String>)> = {
            let mut stmt = db.prepare("SELECT id, message_id FROM ai_runs WHERE session_id=?1")?;
            let rows = stmt
                .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        // One transaction: a crash cannot leave the session half-deleted
        let tx = db.unchecked_transaction()?;
        tx.execute("DELETE FROM ai_sessions WHERE id=?1", [id])?;
        tx.execute("DELETE FROM ai_messages WHERE session_id=?1", [id])?;
        tx.execute(
            "UPDATE ai_runs SET session_id=NULL, message_id=NULL WHERE session_id=?1",
            [id],
        )?;
        tx.commit()?;
        Ok(Some(AiSessionSnapshot {
            session,
            messages,
            run_links,
        }))
    }

    /// Undo of [`Library::delete_ai_session`]: reinsert the session, its
    /// transcript, and the run back-links, with original ids and timestamps.
    pub fn restore_ai_session(&self, snap: &AiSessionSnapshot) -> Result<()> {
        let db = self.db.lock().unwrap();
        let tx = db.unchecked_transaction()?;
        let s = &snap.session;
        tx.execute(
            "INSERT OR REPLACE INTO ai_sessions(id, title, supply, model, effort, agent_session_id, created_at, updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                s.id,
                s.title,
                s.supply,
                s.model,
                s.effort,
                s.agent_session_id,
                s.created_at,
                s.updated_at
            ],
        )?;
        for m in &snap.messages {
            let actions_json = serde_json::to_string(&m.actions).unwrap_or_else(|_| "[]".into());
            tx.execute(
                "INSERT OR REPLACE INTO ai_messages(id, session_id, role, content, actions, created_at)
                 VALUES(?1,?2,?3,?4,?5,?6)",
                params![m.id, m.session_id, m.role, m.content, actions_json, m.created_at],
            )?;
        }
        for (run_id, message_id) in &snap.run_links {
            tx.execute(
                "UPDATE ai_runs SET session_id=?2, message_id=?3 WHERE id=?1",
                params![run_id, s.id, message_id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    // ---------- Messages ----------

    /// Append a turn. The first user message titles the session; every append
    /// bumps the session's activity timestamp (list order).
    pub fn append_ai_message(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        actions: &[String],
    ) -> Result<AiMessage> {
        let id = uuid::Uuid::new_v4().to_string();
        let t = now();
        let actions_json = serde_json::to_string(actions).unwrap_or_else(|_| "[]".into());
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO ai_messages(id, session_id, role, content, actions, created_at)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![id, session_id, role, content, actions_json, t],
        )?;
        db.execute(
            "UPDATE ai_sessions SET updated_at=?2 WHERE id=?1",
            params![session_id, t],
        )?;
        if role == "user" {
            let title: String = content.trim().chars().take(40).collect();
            db.execute(
                "UPDATE ai_sessions SET title=?2 WHERE id=?1 AND title=''",
                params![session_id, title],
            )?;
        }
        Ok(AiMessage {
            id,
            session_id: session_id.to_string(),
            role: role.to_string(),
            content: content.to_string(),
            actions: actions.to_vec(),
            created_at: t,
        })
    }

    /// Oldest first — ready to render (or to replay as BYOK context).
    pub fn list_ai_messages(&self, session_id: &str) -> Result<Vec<AiMessage>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, session_id, role, content, actions, created_at
             FROM ai_messages WHERE session_id=?1 ORDER BY created_at ASC, rowid ASC",
        )?;
        let rows = stmt
            .query_map([session_id], |r| {
                let actions_json: String = r.get(4)?;
                Ok(AiMessage {
                    id: r.get(0)?,
                    session_id: r.get(1)?,
                    role: r.get(2)?,
                    content: r.get(3)?,
                    actions: serde_json::from_str(&actions_json).unwrap_or_default(),
                    created_at: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ---------- Runs (write ledger) ----------

    pub fn record_ai_run(&self, r: &NewAiRun) -> Result<AiRunRecord> {
        let id = uuid::Uuid::new_v4().to_string();
        let t = now();
        let db = self.db.lock().unwrap();
        db.execute(
            &format!(
                "INSERT INTO ai_runs({RUN_COLS}) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)"
            ),
            params![
                id,
                r.asset_id,
                r.kind,
                r.supply,
                r.model,
                r.instruction,
                r.status,
                r.ver,
                r.report,
                r.error,
                r.session_id,
                r.message_id,
                t
            ],
        )?;
        Ok(AiRunRecord {
            id,
            asset_id: r.asset_id.clone(),
            kind: r.kind.clone(),
            supply: r.supply.clone(),
            model: r.model.clone(),
            instruction: r.instruction.clone(),
            status: r.status.clone(),
            ver: r.ver,
            report: r.report.clone(),
            error: r.error.clone(),
            session_id: r.session_id.clone(),
            message_id: r.message_id.clone(),
            created_at: t,
        })
    }

    /// Newest first; capped — the timeline shows recent activity, not archives.
    pub fn list_ai_runs(&self, asset_id: &str, limit: i64) -> Result<Vec<AiRunRecord>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(&format!(
            "SELECT {RUN_COLS} FROM ai_runs WHERE asset_id=?1 ORDER BY created_at DESC, rowid DESC LIMIT ?2"
        ))?;
        let rows = stmt
            .query_map(params![asset_id, limit], row_to_run)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}
