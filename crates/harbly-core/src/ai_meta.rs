//! AI run records + the single write-back path for AI-produced content.
//!
//! A "run" is one task the user fired at an asset (revise / review). Revise
//! runs point at the version they produced; review runs carry the report
//! text. Together with the versions table they reconstruct the asset's AI
//! timeline. The engine itself lives in the separate harbly-ai crate — this
//! module is the only place AI output enters the library.

use crate::error::Result;
use crate::{now, Library};
use rusqlite::params;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiRunRecord {
    pub id: String,
    pub asset_id: String,
    /// "revise" | "review"
    pub kind: String,
    /// Supply id: "claude" | "codex" | "anthropic" | "openai" | "openrouter"
    pub supply: String,
    /// BYOK model name; empty for local agents (they pick their own).
    pub model: String,
    pub instruction: String,
    /// "ok" | "error" | "cancelled"
    pub status: String,
    /// Version produced by a revise run (None: review / no change / failed).
    pub ver: Option<i64>,
    /// Review report (markdown) — kept on the run, shown on the timeline.
    pub report: Option<String>,
    pub error: Option<String>,
    pub created_at: i64,
}

/// Everything the caller knows about a finished run; id/timestamp are minted here.
#[derive(Debug, Clone)]
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
}

impl Library {
    /// Write AI-revised content to the asset file and append a version in one
    /// step. No confirmation gate by design: the version chain IS the safety
    /// net (⌘Z-grade undo via rollback), matching the product's undo-over-
    /// confirm rule. Returns the version number the content landed as.
    pub fn apply_ai_output(&self, id: &str, text: &str, label: &str) -> Result<i64> {
        self.write_asset_text(id, text)?;
        let content = text.as_bytes();
        let hash = blake3::hash(content).to_hex().to_string();
        self.write_version(id, content, &hash, label)
    }

    pub fn record_ai_run(&self, r: &NewAiRun) -> Result<AiRunRecord> {
        let id = uuid::Uuid::new_v4().to_string();
        let t = now();
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO ai_runs(id, asset_id, kind, supply, model, instruction, status, ver, report, error, created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                id, r.asset_id, r.kind, r.supply, r.model, r.instruction, r.status, r.ver,
                r.report, r.error, t
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
            created_at: t,
        })
    }

    /// Newest first; capped — the timeline shows recent activity, not archives.
    pub fn list_ai_runs(&self, asset_id: &str, limit: i64) -> Result<Vec<AiRunRecord>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, asset_id, kind, supply, model, instruction, status, ver, report, error, created_at
             FROM ai_runs WHERE asset_id=?1 ORDER BY created_at DESC, rowid DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![asset_id, limit], |r| {
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
                    created_at: r.get(10)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}
