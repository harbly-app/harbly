use crate::error::Result;
use crate::extract::Extracted;
use crate::types::*;
use crate::{
    asset_kind, extract, file_stem, hdoc, is_asset, is_hidden_component, markdown, now,
    parent_folder, unique_name, AssetKind, Library,
};
use rusqlite::{params, OptionalExtension};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use uuid::Uuid;
use walkdir::WalkDir;

/// Extract title + indexable body from raw content, dispatched by file kind.
fn extract_for(rel: &str, text: &str) -> Extracted {
    match asset_kind(Path::new(rel)) {
        Some(AssetKind::Markdown) => markdown::extract_md(text),
        Some(AssetKind::Hdoc) => hdoc::extract_hdoc(text),
        _ => extract::extract_html(text),
    }
}

impl Library {
    /// Full scan. Incremental strategy: rel_path already registered and (size, mtime) unchanged → skip re-hashing.
    /// Detects three kinds of external change: new files, content modified externally (appends a new version), and Finder moves (path rebound by hash).
    pub fn scan(&self, mut progress: impl FnMut(ScanProgress)) -> Result<ScanSummary> {
        let mut sum = ScanSummary::default();
        let mut seen: HashSet<String> = HashSet::new();

        let files: Vec<(String, i64, i64)> = WalkDir::new(&self.root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| e.depth() == 0 || !is_hidden_component(e.file_name()))
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file() && is_asset(e.path()))
            .filter_map(|e| {
                let rel = e
                    .path()
                    .strip_prefix(&self.root)
                    .ok()?
                    .to_str()?
                    .replace('\\', "/");
                let md = e.metadata().ok()?;
                let mtime = md
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                Some((rel, md.len() as i64, mtime))
            })
            .collect();

        sum.total = files.len();
        let found = files.len();
        let mut indexed = 0usize;

        for (rel, size, mtime) in files {
            seen.insert(rel.clone());
            // A single file failing (e.g. deleted mid-read) must not abort the whole scan
            if let Err(_e) = self.index_path(&rel, size, mtime, &mut sum) {
                continue;
            }
            indexed += 1;
            if indexed.is_multiple_of(25) {
                progress(ScanProgress { found, indexed });
            }
        }
        progress(ScanProgress { found, indexed });

        // Clean up records whose files no longer exist on disk (deleted externally / moved out of the library)
        let stale: Vec<String> = {
            let db = self.db.lock().unwrap();
            let mut stmt = db.prepare("SELECT id, rel_path FROM assets")?;
            let rows: Vec<(String, String)> = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .filter_map(|r| r.ok())
                .collect();
            rows.into_iter()
                .filter(|(_, rel)| !seen.contains(rel))
                .map(|(id, _)| id)
                .collect()
        };
        for id in stale {
            self.remove_asset_rows(&id, true)?;
            sum.removed += 1;
        }

        // The source of truth for tags is the file xattr (interop with Finder): adopt disk-side changes into the index
        #[cfg(target_os = "macos")]
        {
            self.migrate_db_tags_to_xattr()?;
            sum.tags_synced = self.sync_tags_from_disk()?;
        }
        Ok(sum)
    }

    /// One-time migration: early versions stored tags only in the database — write them out to file xattrs;
    /// from then on the xattr is the source of truth for tags (Finder-side edits get adopted during scans).
    #[cfg(target_os = "macos")]
    fn migrate_db_tags_to_xattr(&self) -> Result<()> {
        let done: Option<String> = {
            let db = self.db.lock().unwrap();
            db.query_row(
                "SELECT value FROM meta WHERE key='tags_in_xattr'",
                [],
                |r| r.get(0),
            )
            .optional()?
        };
        if done.is_some() {
            return Ok(());
        }
        for a in self.all_assets()? {
            if !a.tags.is_empty() {
                let abs = self.abs(&a.rel_path);
                if abs.exists() && crate::tags_xattr::read_tags(&abs).is_empty() {
                    let _ = crate::tags_xattr::write_tags(&abs, &a.tags);
                }
            }
        }
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES('tags_in_xattr','1')",
            [],
        )?;
        Ok(())
    }

    /// Make database tags follow the on-disk xattrs (Finder adding/removing tags, restoring files that carry tags, etc.)
    #[cfg(target_os = "macos")]
    fn sync_tags_from_disk(&self) -> Result<usize> {
        let mut n = 0usize;
        for a in self.all_assets()? {
            let disk = crate::tags_xattr::read_tags(&self.abs(&a.rel_path));
            let mut want = disk.clone();
            want.sort();
            let mut have = a.tags.clone();
            have.sort();
            have.dedup();
            if want != have {
                self.set_tags_db(&a.id, &disk)?;
                n += 1;
            }
        }
        Ok(n)
    }

    fn index_path(&self, rel: &str, size: i64, mtime: i64, sum: &mut ScanSummary) -> Result<()> {
        let existing: Option<(String, i64, i64, String)> = {
            let db = self.db.lock().unwrap();
            db.query_row(
                "SELECT id, size_bytes, mtime, current_hash FROM assets WHERE rel_path=?1",
                [rel],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?
        };
        if let Some((_, s, m, _)) = &existing {
            if *s == size && *m == mtime {
                return Ok(());
            }
        }

        let content = std::fs::read(self.abs(rel))?;
        let hash = blake3::hash(&content).to_hex().to_string();

        match existing {
            Some((id, _, _, old_hash)) => {
                if old_hash == hash {
                    let db = self.db.lock().unwrap();
                    db.execute(
                        "UPDATE assets SET size_bytes=?1, mtime=?2 WHERE id=?3",
                        params![size, mtime, id],
                    )?;
                } else {
                    self.update_asset_content(&id, &content, &hash, mtime, "外部修改")?;
                    sum.updated += 1;
                }
            }
            None => {
                // New path: hash matches an in-library asset whose old path is gone → Finder move; rebind the path without losing history
                let moved: Option<(String, String)> = {
                    let db = self.db.lock().unwrap();
                    db.query_row(
                        "SELECT id, rel_path FROM assets WHERE current_hash=?1 LIMIT 1",
                        [&hash],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?
                };
                if let Some((id, old_rel)) = moved {
                    if !self.abs(&old_rel).exists() {
                        let db = self.db.lock().unwrap();
                        db.execute(
                            "UPDATE assets SET rel_path=?1, folder=?2, mtime=?3, updated_at=?4 WHERE id=?5",
                            params![rel, parent_folder(rel), mtime, now(), id],
                        )?;
                        sum.moved += 1;
                        return Ok(());
                    }
                }
                self.insert_new_asset(rel, &content, &hash, size, mtime, "import", "初始导入")?;
                sum.added += 1;
            }
        }
        Ok(())
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "internal write path; a params struct would just mirror the columns"
    )]
    pub(crate) fn insert_new_asset(
        &self,
        rel: &str,
        content: &[u8],
        hash: &str,
        size: i64,
        mtime: i64,
        source: &str,
        ver_label: &str,
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let text = String::from_utf8_lossy(content);
        let ex = extract_for(rel, &text);
        let title = ex.title.unwrap_or_else(|| file_stem(rel));
        let t = now();
        {
            let db = self.db.lock().unwrap();
            db.execute(
                "INSERT INTO assets(id, rel_path, folder, title, source, current_hash, size_bytes, mtime, created_at, updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)",
                params![id, rel, parent_folder(rel), title, source, hash, size, mtime, t],
            )?;
            db.execute(
                "INSERT INTO fts(asset_id, title, body) VALUES(?1,?2,?3)",
                params![id, self.seg(&title), self.seg(&ex.body)],
            )?;
        }
        self.write_version(&id, content, hash, ver_label)?;
        // If a newly registered file carries Finder tags (import / restored from Trash / moved in externally), index them immediately
        #[cfg(target_os = "macos")]
        {
            let disk = crate::tags_xattr::read_tags(&self.abs(rel));
            if !disk.is_empty() {
                self.set_tags_db(&id, &disk)?;
            }
        }
        Ok(id)
    }

    /// Content change (external edit / rollback): update metadata and index, and append a full version
    pub(crate) fn update_asset_content(
        &self,
        id: &str,
        content: &[u8],
        hash: &str,
        mtime: i64,
        ver_label: &str,
    ) -> Result<()> {
        let rel: String = {
            let db = self.db.lock().unwrap();
            db.query_row("SELECT rel_path FROM assets WHERE id=?1", [id], |r| {
                r.get(0)
            })?
        };
        self.reindex_content(id, &rel, content, hash, mtime, Some(ver_label))
    }

    /// Update an asset's row + FTS index from freshly written bytes, optionally
    /// appending a version snapshot. Autosave passes `version_label = None`
    /// (per-keystroke saves never version); external edits and rollbacks pass a
    /// label so the change enters the version chain.
    pub(crate) fn reindex_content(
        &self,
        id: &str,
        rel: &str,
        content: &[u8],
        hash: &str,
        mtime: i64,
        version_label: Option<&str>,
    ) -> Result<()> {
        let text = String::from_utf8_lossy(content);
        let ex = extract_for(rel, &text);
        {
            let db = self.db.lock().unwrap();
            let title: String = match ex.title {
                Some(t) => t,
                None => db.query_row("SELECT title FROM assets WHERE id=?1", [id], |r| r.get(0))?,
            };
            db.execute(
                "UPDATE assets SET title=?1, current_hash=?2, size_bytes=?3, mtime=?4, updated_at=?5 WHERE id=?6",
                params![title, hash, content.len() as i64, mtime, now(), id],
            )?;
            db.execute("DELETE FROM fts WHERE asset_id=?1", [id])?;
            db.execute(
                "INSERT INTO fts(asset_id, title, body) VALUES(?1,?2,?3)",
                params![id, self.seg(&title), self.seg(&ex.body)],
            )?;
        }
        if let Some(label) = version_label {
            self.write_version(id, content, hash, label)?;
        }
        Ok(())
    }

    /// A version = a full file, stored at .harbly/versions/<asset_id>/vN.<ext>.
    /// Deduplicated: if the newest version already carries this hash, no new
    /// snapshot is written (keeps checkpoints idempotent and absorbs the
    /// autosave/scan race that could otherwise append a spurious version).
    pub(crate) fn write_version(
        &self,
        asset_id: &str,
        content: &[u8],
        hash: &str,
        label: &str,
    ) -> Result<i64> {
        let ext = self.asset_ext(asset_id);
        let dir = self.versions_dir().join(asset_id);
        std::fs::create_dir_all(&dir)?;
        // Dedup check + number allocation happen in ONE statement under the
        // database's write serialization: concurrent writers (a second AI
        // turn in-process, or the harbly-mcp process) can never both claim
        // the same slot — the old read-then-insert split could fail its
        // INSERT after the file was already on disk.
        let next = {
            let db = self.db.lock().unwrap();
            let last: Option<(i64, String)> = db
                .query_row(
                    "SELECT ver, hash FROM versions WHERE asset_id=?1 ORDER BY ver DESC LIMIT 1",
                    [asset_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((ver, h)) = last {
                if h == hash {
                    return Ok(ver);
                }
            }
            db.query_row(
                "INSERT INTO versions(asset_id, ver, hash, label, size_bytes, created_at)
                 SELECT ?1, COALESCE(MAX(ver),0)+1, ?2, ?3, ?4, ?5 FROM versions WHERE asset_id=?1
                 RETURNING ver",
                params![asset_id, hash, label, content.len() as i64, now()],
                |r| r.get(0),
            )?
        };
        // Snapshot file after the row is reserved (distinct numbers → distinct
        // files, no clobbering); roll the row back if the disk write fails so
        // no phantom version remains.
        if let Err(e) = std::fs::write(dir.join(format!("v{next}.{ext}")), content) {
            let db = self.db.lock().unwrap();
            let _ = db.execute(
                "DELETE FROM versions WHERE asset_id=?1 AND ver=?2",
                params![asset_id, next],
            );
            return Err(e.into());
        }
        Ok(next)
    }

    /// Drop version rows whose snapshot file is missing. A hard kill between
    /// write_version's row INSERT and its file write leaves such a phantom:
    /// its preview 404s, rollback fails forever, and the dedup check would
    /// match its hash without ever materializing a file. Runs at library
    /// open; rows younger than a few seconds are skipped so a write that is
    /// mid-flight in ANOTHER process is never swept.
    pub(crate) fn sweep_orphan_versions(&self) -> Result<()> {
        let cutoff = now() - 5;
        let rows: Vec<(String, i64)> = {
            let db = self.db.lock().unwrap();
            let mut stmt =
                db.prepare("SELECT asset_id, ver FROM versions WHERE created_at <= ?1")?;
            let rows = stmt
                .query_map([cutoff], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        let dead: Vec<&(String, i64)> = rows
            .iter()
            .filter(|(id, ver)| !self.version_file_path(id, *ver).is_file())
            .collect();
        if !dead.is_empty() {
            let db = self.db.lock().unwrap();
            for (id, ver) in dead {
                db.execute(
                    "DELETE FROM versions WHERE asset_id=?1 AND ver=?2",
                    params![id, ver],
                )?;
            }
        }
        Ok(())
    }

    pub(crate) fn remove_asset_rows(&self, id: &str, delete_version_files: bool) -> Result<()> {
        {
            let db = self.db.lock().unwrap();
            db.execute("DELETE FROM assets WHERE id=?1", [id])?;
            db.execute("DELETE FROM versions WHERE asset_id=?1", [id])?;
            db.execute("DELETE FROM fts WHERE asset_id=?1", [id])?;
            db.execute("DELETE FROM asset_tags WHERE asset_id=?1", [id])?;
            db.execute("DELETE FROM ai_runs WHERE asset_id=?1", [id])?;
        }
        if delete_version_files {
            let _ = std::fs::remove_dir_all(self.versions_dir().join(id));
        }
        Ok(())
    }

    /// Import external files: dedupe by content hash, auto-suffix on name collision
    pub fn import_files(&self, paths: &[PathBuf], dest_rel: &str) -> Result<ImportResult> {
        let mut res = ImportResult::default();
        let dest_dir = if dest_rel.is_empty() {
            self.root().to_path_buf()
        } else {
            self.abs(dest_rel)
        };
        std::fs::create_dir_all(&dest_dir)?;

        for p in paths {
            if !is_asset(p) {
                res.skipped += 1;
                continue;
            }
            let content = match std::fs::read(p) {
                Ok(c) => c,
                Err(_) => {
                    res.skipped += 1;
                    continue;
                }
            };
            let hash = blake3::hash(&content).to_hex().to_string();
            let dup: Option<String> = {
                let db = self.db.lock().unwrap();
                db.query_row(
                    "SELECT id FROM assets WHERE current_hash=?1 LIMIT 1",
                    [&hash],
                    |r| r.get(0),
                )
                .optional()?
            };
            if let Some(existing) = dup {
                res.duplicates += 1;
                res.dup_of.push(existing);
                continue;
            }
            let orig_name = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("导入.html")
                .replace('/', "-");
            let name = unique_name(&dest_dir, &orig_name);
            if name != orig_name {
                res.renamed += 1;
            }
            let dest = dest_dir.join(&name);
            std::fs::write(&dest, &content)?;
            let _ = crate::tags_xattr::copy_tags(p, &dest); // the source file's Finder tags are kept through import
            let md = std::fs::metadata(&dest)?;
            let mtime = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let rel = if dest_rel.is_empty() {
                name.clone()
            } else {
                format!("{dest_rel}/{name}")
            };
            self.insert_new_asset(
                &rel,
                &content,
                &hash,
                content.len() as i64,
                mtime,
                "import",
                "初始导入",
            )?;
            res.imported.push(rel);
            res.added += 1;
        }
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn sweep_drops_fileless_version_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = crate::Library::open_or_create(tmp.path().join("L")).unwrap();
        std::fs::write(
            lib.root().join("a.html"),
            "<html><title>A</title><body>x</body></html>",
        )
        .unwrap();
        lib.scan(|_| {}).unwrap();
        let a = lib
            .list_assets("", crate::SortKey::Recent)
            .unwrap()
            .remove(0);
        // Simulate the crash leftover: a version row whose file never landed
        // (created_at 0 puts it safely past the mid-flight grace window)
        {
            let db = lib.db.lock().unwrap();
            db.execute(
                "INSERT INTO versions(asset_id, ver, hash, label, size_bytes, created_at)
                 VALUES(?1, 99, 'nohash', 'AI 改版', 1, 0)",
                rusqlite::params![a.id],
            )
            .unwrap();
        }
        assert_eq!(lib.list_versions(&a.id).unwrap().len(), 2);
        lib.sweep_orphan_versions().unwrap();
        let vs = lib.list_versions(&a.id).unwrap();
        assert_eq!(vs.len(), 1);
        // The real v1 (its file exists) survives the sweep
        assert!(lib.version_file_path(&a.id, 1).is_file());
    }
}
