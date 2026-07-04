use crate::error::{HarblyError, Result};
use crate::types::{TreeFile, *};
use crate::{is_hidden_component, now, parent_folder, unique_name, Library, INBOX_DIR};
use rusqlite::params;
use std::collections::HashMap;
use walkdir::WalkDir;

const ASSET_COLS: &str = "a.id, a.rel_path, a.folder, a.title, a.source, a.size_bytes, a.current_hash, a.created_at, a.updated_at, \
    (SELECT COUNT(*) FROM versions v WHERE v.asset_id=a.id), \
    (SELECT GROUP_CONCAT(tag, char(31)) FROM asset_tags t WHERE t.asset_id=a.id)";

/// Recursively copy a directory, skipping hidden entries (.harbly etc.); Finder tag xattrs travel along
pub fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    let _ = crate::tags_xattr::copy_tags(src, dst);
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        if is_hidden_component(&entry.file_name()) {
            continue;
        }
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
            let _ = crate::tags_xattr::copy_tags(&entry.path(), &to);
        }
    }
    Ok(())
}

fn row_to_asset(r: &rusqlite::Row) -> rusqlite::Result<AssetMeta> {
    let rel: String = r.get(1)?;
    let file_name = rel.rsplit('/').next().unwrap_or(rel.as_str()).to_string();
    let tags_raw: Option<String> = r.get(10)?;
    Ok(AssetMeta {
        id: r.get(0)?,
        file_name,
        rel_path: rel,
        folder: r.get(2)?,
        title: r.get(3)?,
        source: r.get(4)?,
        size_bytes: r.get(5)?,
        current_hash: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
        ver_count: r.get(9)?,
        tags: tags_raw
            .map(|s| s.split('\u{1f}').map(|t| t.to_string()).collect())
            .unwrap_or_default(),
    })
}

impl Library {
    pub fn asset(&self, id: &str) -> Result<AssetMeta> {
        let db = self.db.lock().unwrap();
        let sql = format!("SELECT {ASSET_COLS} FROM assets a WHERE a.id=?1");
        Ok(db.query_row(&sql, [id], row_to_asset)?)
    }

    pub fn asset_by_rel(&self, rel: &str) -> Result<AssetMeta> {
        let db = self.db.lock().unwrap();
        let sql = format!("SELECT {ASSET_COLS} FROM assets a WHERE a.rel_path=?1");
        Ok(db.query_row(&sql, [rel], row_to_asset)?)
    }

    /// When the target rel already exists, produce an available suffixed relative path
    pub fn unique_rel(&self, rel: &str) -> String {
        let (folder, name) = rel
            .rsplit_once('/')
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .unwrap_or((String::new(), rel.to_string()));
        let dir = if folder.is_empty() {
            self.root().to_path_buf()
        } else {
            self.abs(&folder)
        };
        let n = unique_name(&dir, &name);
        if folder.is_empty() {
            n
        } else {
            format!("{folder}/{n}")
        }
    }

    pub fn asset_abs_path(&self, id: &str) -> Result<std::path::PathBuf> {
        let rel: String = {
            let db = self.db.lock().unwrap();
            db.query_row("SELECT rel_path FROM assets WHERE id=?1", [id], |r| {
                r.get(0)
            })?
        };
        Ok(self.abs(&rel))
    }

    /// Empty folder string = the "All Assets" view: list the whole library recursively (inbox excluded);
    /// a concrete folder lists only its direct files (Finder convention — subfolder contents are reached via the sidebar tree)
    pub fn list_assets(&self, folder: &str, sort: SortKey) -> Result<Vec<AssetMeta>> {
        let order = match sort {
            SortKey::Recent => "a.created_at DESC",
            SortKey::Name => "a.title COLLATE NOCASE ASC",
            SortKey::Modified => "a.updated_at DESC",
        };
        let (cond, param) = if folder.is_empty() {
            ("a.folder != ?1", INBOX_DIR)
        } else {
            ("a.folder = ?1", folder)
        };
        let db = self.db.lock().unwrap();
        let sql = format!("SELECT {ASSET_COLS} FROM assets a WHERE {cond} ORDER BY {order}");
        let mut stmt = db.prepare(&sql)?;
        let rows = stmt
            .query_map([param], row_to_asset)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn inbox_count(&self) -> Result<i64> {
        let db = self.db.lock().unwrap();
        Ok(db.query_row(
            "SELECT COUNT(*) FROM assets WHERE folder=?1",
            [INBOX_DIR],
            |r| r.get(0),
        )?)
    }

    pub fn total_count(&self) -> Result<i64> {
        let db = self.db.lock().unwrap();
        Ok(db.query_row("SELECT COUNT(*) FROM assets", [], |r| r.get(0))?)
    }

    pub fn all_assets(&self) -> Result<Vec<AssetMeta>> {
        let db = self.db.lock().unwrap();
        let sql = format!("SELECT {ASSET_COLS} FROM assets a ORDER BY a.created_at DESC");
        let mut stmt = db.prepare(&sql)?;
        let rows = stmt
            .query_map([], row_to_asset)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Directory tree = the real on-disk directories (excluding .harbly, hidden directories, and _inbox); counts are recursive asset counts
    pub fn dir_tree(&self) -> Result<TreeNode> {
        let mut dirs: Vec<String> = WalkDir::new(self.root())
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                e.depth() == 0
                    || (e.file_type().is_dir()
                        && !is_hidden_component(e.file_name())
                        && e.file_name() != INBOX_DIR)
            })
            .filter_map(|e| e.ok())
            .filter(|e| e.depth() > 0 && e.file_type().is_dir())
            .filter_map(|e| {
                Some(
                    e.path()
                        .strip_prefix(self.root())
                        .ok()?
                        .to_str()?
                        .replace('\\', "/"),
                )
            })
            .collect();
        dirs.sort();

        // Direct asset count per folder
        let direct: HashMap<String, i64> = {
            let db = self.db.lock().unwrap();
            let mut stmt = db.prepare("SELECT folder, COUNT(*) FROM assets GROUP BY folder")?;
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter().collect()
        };

        // Files directly inside each folder (inbox excluded), sorted by name
        let mut files_of: HashMap<String, Vec<TreeFile>> = HashMap::new();
        {
            let db = self.db.lock().unwrap();
            let mut stmt = db.prepare(
                "SELECT id, rel_path, folder, (SELECT COUNT(*) FROM versions v WHERE v.asset_id=assets.id) \
                 FROM assets WHERE folder != ?1 ORDER BY rel_path COLLATE NOCASE",
            )?;
            let rows = stmt
                .query_map([INBOX_DIR], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (id, rel, folder, vc) in rows {
                let name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
                files_of.entry(folder).or_default().push(TreeFile {
                    id,
                    name,
                    ver_count: vc,
                });
            }
        }

        let mut children_of: HashMap<String, Vec<String>> = HashMap::new();
        for d in &dirs {
            children_of
                .entry(parent_folder(d))
                .or_default()
                .push(d.clone());
        }

        fn build(
            rel: &str,
            name: &str,
            children_of: &HashMap<String, Vec<String>>,
            direct: &HashMap<String, i64>,
            files_of: &mut HashMap<String, Vec<TreeFile>>,
        ) -> TreeNode {
            let kids: Vec<TreeNode> = children_of
                .get(rel)
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|c| {
                    build(
                        c,
                        c.rsplit('/').next().unwrap_or(c),
                        children_of,
                        direct,
                        files_of,
                    )
                })
                .collect();
            let count =
                direct.get(rel).copied().unwrap_or(0) + kids.iter().map(|k| k.count).sum::<i64>();
            TreeNode {
                name: name.to_string(),
                rel: rel.to_string(),
                count,
                children: kids,
                files: files_of.remove(rel).unwrap_or_default(),
            }
        }
        Ok(build("", "", &children_of, &direct, &mut files_of))
    }

    pub fn create_folder(&self, parent_rel: &str, name: &str) -> Result<String> {
        let name = name.trim().replace('/', "-");
        if name.is_empty() || name.starts_with('.') {
            return Err(HarblyError::msg("无效的文件夹名"));
        }
        let rel = if parent_rel.is_empty() {
            name
        } else {
            format!("{parent_rel}/{name}")
        };
        std::fs::create_dir_all(self.abs(&rel))?;
        Ok(rel)
    }

    fn guard_folder(&self, rel: &str) -> Result<()> {
        if rel.is_empty() || rel == INBOX_DIR || rel.split('/').any(|c| c == "..") {
            return Err(HarblyError::msg("不能对此目录执行该操作"));
        }
        if !self.abs(rel).is_dir() {
            return Err(HarblyError::msg("文件夹不存在"));
        }
        Ok(())
    }

    /// Rename a folder: rename on disk, then rescan — scan rebinds the files to their new paths by content hash, so history and thumbnails survive
    pub fn rename_folder(&self, rel: &str, new_name: &str) -> Result<String> {
        self.guard_folder(rel)?;
        let name = new_name.trim().replace('/', "-");
        if name.is_empty() || name.starts_with('.') {
            return Err(HarblyError::msg("无效的文件夹名"));
        }
        let parent = parent_folder(rel);
        let new_rel = if parent.is_empty() {
            name
        } else {
            format!("{parent}/{name}")
        };
        if new_rel == rel {
            return Ok(new_rel);
        }
        if self.abs(&new_rel).exists() {
            return Err(HarblyError::msg("同名文件夹已存在"));
        }
        std::fs::rename(self.abs(rel), self.abs(&new_rel))?;
        self.scan(|_| {})?;
        Ok(new_rel)
    }

    /// Delete a folder: move it wholesale to the system Trash (recoverable), then rescan to clean up the index
    pub fn delete_folder(&self, rel: &str) -> Result<()> {
        self.guard_folder(rel)?;
        trash::delete(self.abs(rel))?;
        self.scan(|_| {})?;
        Ok(())
    }

    /// Duplicate a folder: recursive copy named "xxx 副本" (the literal on-disk "copy" suffix), rescan to register
    pub fn duplicate_folder(&self, rel: &str) -> Result<String> {
        self.guard_folder(rel)?;
        let name = rel.rsplit('/').next().unwrap_or(rel);
        let parent = parent_folder(rel);
        let parent_dir = if parent.is_empty() {
            self.root().to_path_buf()
        } else {
            self.abs(&parent)
        };
        let new_name = unique_name(&parent_dir, &format!("{name} 副本"));
        let new_rel = if parent.is_empty() {
            new_name
        } else {
            format!("{parent}/{new_name}")
        };
        copy_dir_recursive(&self.abs(rel), &self.abs(&new_rel))?;
        self.scan(|_| {})?;
        Ok(new_rel)
    }

    /// Duplicate a file as "xxx 副本.html" (the literal "copy" suffix, same directory)
    pub fn duplicate_asset(&self, id: &str) -> Result<AssetMeta> {
        let cur = self.asset(id)?;
        let content = std::fs::read(self.abs(&cur.rel_path))?;
        let dir = if cur.folder.is_empty() {
            self.root().to_path_buf()
        } else {
            self.abs(&cur.folder)
        };
        let (stem, ext) = match cur.file_name.rsplit_once('.') {
            Some((s, e)) => (s.to_string(), format!(".{e}")),
            None => (cur.file_name.clone(), String::new()),
        };
        let name = unique_name(&dir, &format!("{stem} 副本{ext}"));
        std::fs::write(dir.join(&name), &content)?;
        let _ = crate::tags_xattr::copy_tags(&self.abs(&cur.rel_path), &dir.join(&name)); // the duplicate inherits tags (same behavior as Finder)
        let rel = if cur.folder.is_empty() {
            name
        } else {
            format!("{}/{}", cur.folder, name)
        };
        let md = std::fs::metadata(self.abs(&rel))?;
        let mtime = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let hash = blake3::hash(&content).to_hex().to_string();
        let new_id = self.insert_new_asset(
            &rel,
            &content,
            &hash,
            content.len() as i64,
            mtime,
            "duplicate",
            "复制副本",
        )?;
        self.asset(&new_id)
    }

    /// Full-text search: the query string goes through jieba segmentation too; the final ASCII token gets prefix matching
    pub fn search(&self, q: &str) -> Result<Vec<SearchHit>> {
        let q = q.trim();
        if q.is_empty() {
            return Ok(vec![]);
        }
        let toks: Vec<String> = self
            .jieba
            .cut_for_search(q, true)
            .into_iter()
            .map(|t| t.trim().replace('"', ""))
            .filter(|t| !t.is_empty())
            .collect();
        if toks.is_empty() {
            return Ok(vec![]);
        }
        let n = toks.len();
        let match_q = toks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                if i == n - 1 && t.is_ascii() {
                    format!("\"{t}\"*")
                } else {
                    format!("\"{t}\"")
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        let db = self.db.lock().unwrap();
        let sql = format!(
            "SELECT {ASSET_COLS}, snippet(fts, 2, '', '', '…', 10) \
             FROM fts JOIN assets a ON a.id = fts.asset_id \
             WHERE fts MATCH ?1 ORDER BY bm25(fts) LIMIT 40"
        );
        let mut stmt = db.prepare(&sql)?;
        let hits = stmt.query_map([&match_q], |r| {
            Ok(SearchHit {
                asset: row_to_asset(r)?,
                snippet: r.get(11)?,
            })
        });
        // MATCH syntax errors caused by special characters are treated as empty results, not errors
        let mut out: Vec<SearchHit> = match hits {
            Ok(iter) => iter.filter_map(|h| h.ok()).collect(),
            Err(_) => vec![],
        };
        // Exact tag hits are merged into the results (placed first)
        {
            let sql = format!(
                "SELECT DISTINCT {ASSET_COLS} FROM asset_tags g JOIN assets a ON a.id=g.asset_id WHERE g.tag=?1 LIMIT 20"
            );
            if let Ok(mut stmt) = db.prepare(&sql) {
                if let Ok(rows) = stmt.query_map([q], row_to_asset) {
                    for a in rows.filter_map(|r| r.ok()) {
                        if !out.iter().any(|h| h.asset.id == a.id) {
                            out.insert(
                                0,
                                SearchHit {
                                    asset: a,
                                    snippet: format!("标签: {q}"),
                                },
                            );
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    /// Set tags: write the file xattr (Finder interop; travels with the file on move/copy/backup) + the database cache.
    /// An xattr write failure (read-only volume etc.) is non-blocking — the database goes first, and the next scan reconciles.
    pub fn set_tags(&self, id: &str, tags: &[String]) -> Result<()> {
        let cur = self.asset(id)?;
        let _ = crate::tags_xattr::write_tags(&self.abs(&cur.rel_path), tags);
        self.set_tags_db(id, tags)
    }

    /// Update only the database cache (used when a scan reconciles with disk, avoiding a loop from writing the xattr back)
    pub(crate) fn set_tags_db(&self, id: &str, tags: &[String]) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute("DELETE FROM asset_tags WHERE asset_id=?1", [id])?;
        for t in tags {
            let t = t.trim();
            if t.is_empty() {
                continue;
            }
            db.execute(
                "INSERT OR IGNORE INTO asset_tags(asset_id, tag) VALUES(?1, ?2)",
                params![id, t],
            )?;
        }
        Ok(())
    }

    pub fn all_tags(&self) -> Result<Vec<TagInfo>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT tag, COUNT(*) FROM asset_tags GROUP BY tag ORDER BY COUNT(*) DESC, tag COLLATE NOCASE",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(TagInfo {
                    name: r.get(0)?,
                    count: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn assets_by_tag(&self, tag: &str) -> Result<Vec<AssetMeta>> {
        let db = self.db.lock().unwrap();
        let sql = format!(
            "SELECT {ASSET_COLS} FROM asset_tags g JOIN assets a ON a.id=g.asset_id WHERE g.tag=?1 ORDER BY a.created_at DESC"
        );
        let mut stmt = db.prepare(&sql)?;
        let rows = stmt
            .query_map([tag], row_to_asset)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Export a folder as a zip (skipping .harbly and other hidden entries)
    pub fn export_folder_zip(&self, rel: &str, dest: &std::path::Path) -> Result<usize> {
        self.guard_folder(rel)?;
        let src = self.abs(rel);
        let file = std::fs::File::create(dest)?;
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default();
        let mut count = 0usize;
        for entry in WalkDir::new(&src)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| e.depth() == 0 || !is_hidden_component(e.file_name()))
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let Ok(inner) = entry.path().strip_prefix(&src) else {
                continue;
            };
            let Some(name) = inner.to_str() else { continue };
            zip.start_file(name.replace('\\', "/"), opts)
                .map_err(|e| HarblyError::Msg(e.to_string()))?;
            let mut f = std::fs::File::open(entry.path())?;
            std::io::copy(&mut f, &mut zip)?;
            count += 1;
        }
        zip.finish().map_err(|e| HarblyError::Msg(e.to_string()))?;
        Ok(count)
    }

    pub fn rename_asset(&self, id: &str, new_name: &str) -> Result<AssetMeta> {
        let mut name = new_name.trim().replace('/', "-");
        if name.is_empty() {
            return Err(HarblyError::msg("名称不能为空"));
        }
        let cur = self.asset(id)?;
        // Renaming never changes an asset's type: drop any managed extension the
        // user typed, then re-append the asset's own current extension.
        if let Some(ext) = crate::ext_of(&cur.rel_path) {
            let lower = name.to_ascii_lowercase();
            for m in ["html", "htm", "md", "markdown"] {
                if lower.ends_with(&format!(".{m}")) {
                    name.truncate(name.len() - (m.len() + 1));
                    break;
                }
            }
            // Reject an extension-only rename (e.g. ".md"): the stem is now empty,
            // and re-appending would produce a hidden, unscannable file like ".md".
            if name.is_empty() {
                return Err(HarblyError::msg("名称不能为空"));
            }
            name.push('.');
            name.push_str(&ext);
        }
        if name == cur.file_name {
            return Ok(cur);
        }
        let new_rel = if cur.folder.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", cur.folder, name)
        };
        if self.abs(&new_rel).exists() {
            return Err(HarblyError::msg("同名文件已存在"));
        }
        std::fs::rename(self.abs(&cur.rel_path), self.abs(&new_rel))?;
        {
            let db = self.db.lock().unwrap();
            db.execute(
                "UPDATE assets SET rel_path=?1, updated_at=?2 WHERE id=?3",
                params![new_rel, now(), id],
            )?;
        }
        self.asset(id)
    }

    /// Move to a target folder ("" = library root, INBOX_DIR = inbox), auto-suffixing on name collision
    pub fn move_asset(&self, id: &str, dest_rel: &str) -> Result<AssetMeta> {
        let cur = self.asset(id)?;
        if cur.folder == dest_rel {
            return Ok(cur);
        }
        let dest_dir = if dest_rel.is_empty() {
            self.root().to_path_buf()
        } else {
            self.abs(dest_rel)
        };
        std::fs::create_dir_all(&dest_dir)?;
        let name = unique_name(&dest_dir, &cur.file_name);
        let new_rel = if dest_rel.is_empty() {
            name.clone()
        } else {
            format!("{dest_rel}/{name}")
        };
        std::fs::rename(self.abs(&cur.rel_path), self.abs(&new_rel))?;
        {
            let db = self.db.lock().unwrap();
            db.execute(
                "UPDATE assets SET rel_path=?1, folder=?2, updated_at=?3 WHERE id=?4",
                params![new_rel, dest_rel, now(), id],
            )?;
        }
        self.asset(id)
    }

    /// Clear only the index records and thumbnail, leaving the disk file alone (the caller has already handled where the file goes)
    pub fn forget_asset(&self, id: &str) -> Result<()> {
        let cur = self.asset(id)?;
        let _ = std::fs::remove_file(self.thumb_path(&cur.current_hash));
        self.remove_asset_rows(id, false)?;
        Ok(())
    }

    /// Delete = move the asset together with all its historical versions to the system Trash (recoverable), and clear the index
    pub fn trash_asset(&self, id: &str) -> Result<()> {
        let cur = self.asset(id)?;
        trash::delete(self.abs(&cur.rel_path))?;
        let vdir = self.versions_dir().join(id);
        if vdir.exists() {
            let _ = trash::delete(&vdir);
        }
        let _ = std::fs::remove_file(self.thumb_path(&cur.current_hash));
        self.remove_asset_rows(id, false)?;
        Ok(())
    }

    pub fn list_versions(&self, asset_id: &str) -> Result<Vec<VersionInfo>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT ver, hash, label, size_bytes, created_at FROM versions WHERE asset_id=?1 ORDER BY ver DESC",
        )?;
        let rows = stmt
            .query_map([asset_id], |r| {
                Ok(VersionInfo {
                    ver: r.get(0)?,
                    hash: r.get(1)?,
                    label: r.get(2)?,
                    size_bytes: r.get(3)?,
                    created_at: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Rollback: write a historical version's content back to the current file, appended as a new version (history is never overwritten)
    pub fn restore_version(&self, id: &str, ver: i64) -> Result<()> {
        let vpath = self.version_file_path(id, ver);
        let content = std::fs::read(&vpath)?;
        let hash = blake3::hash(&content).to_hex().to_string();
        let cur = self.asset(id)?;
        let abs = self.abs(&cur.rel_path);
        std::fs::write(&abs, &content)?;
        let md = std::fs::metadata(&abs)?;
        let mtime = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.update_asset_content(id, &content, &hash, mtime, &format!("回滚到 v{ver}"))?;
        Ok(())
    }

    /// Read an asset's current file contents as text (for the in-app editor).
    pub fn read_asset_text(&self, id: &str) -> Result<String> {
        let abs = self.asset_abs_path(id)?;
        Ok(std::fs::read_to_string(&abs)?)
    }

    /// Write new text to an asset's file and re-index it, WITHOUT creating a
    /// version. The editor autosaves through this on every debounced keystroke;
    /// a single version is captured per session via [`Library::checkpoint_version`].
    /// The write is atomic (same-dir temp file + rename) and preserves Finder tags.
    pub fn write_asset_text(&self, id: &str, text: &str) -> Result<AssetMeta> {
        let cur = self.asset(id)?;
        let abs = self.abs(&cur.rel_path);
        let dir = abs
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.root().to_path_buf());
        // The dot prefix keeps the file watcher quiet until the final rename, so a
        // scan never observes the half-written temp file.
        let tmp = dir.join(format!(
            ".{}.tmp-{}",
            cur.file_name,
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::write(&tmp, text.as_bytes())?;
        // std::fs::rename does not carry xattrs from the destination; copy the
        // asset's Finder tags onto the temp file so the atomic replace keeps them.
        let _ = crate::tags_xattr::copy_tags(&abs, &tmp);
        if let Err(e) = std::fs::rename(&tmp, &abs) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        let content = text.as_bytes();
        let hash = blake3::hash(content).to_hex().to_string();
        let md = std::fs::metadata(&abs)?;
        self.reindex_content(
            id,
            &cur.rel_path,
            content,
            &hash,
            crate::mtime_secs(&md),
            None,
        )?;
        self.asset(id)
    }

    /// Append a version snapshot if the file changed since `base_hash` (captured
    /// when the editing session opened). Called once when the session ends; the
    /// dedup guard in `write_version` makes repeated calls idempotent. Returns the
    /// new version number, or `None` if nothing changed.
    pub fn checkpoint_version(&self, id: &str, base_hash: &str) -> Result<Option<i64>> {
        let cur = self.asset(id)?;
        if cur.current_hash == base_hash {
            return Ok(None);
        }
        let content = std::fs::read(self.abs(&cur.rel_path))?;
        let hash = blake3::hash(&content).to_hex().to_string();
        let ver = self.write_version(id, &content, &hash, "编辑")?;
        Ok(Some(ver))
    }

    /// Create a new empty Markdown asset in `folder` and register it. `name_stem`
    /// is the (already localized) base name; a ".md" extension and a uniqueness
    /// suffix are applied here.
    pub fn create_markdown_asset(&self, folder: &str, name_stem: &str) -> Result<AssetMeta> {
        let dir = if folder.is_empty() {
            self.root().to_path_buf()
        } else {
            self.abs(folder)
        };
        std::fs::create_dir_all(&dir)?;
        let mut stem = name_stem.trim().replace('/', "-");
        if stem.is_empty() {
            stem = "Untitled".to_string();
        }
        let name = unique_name(&dir, &format!("{stem}.md"));
        let abs = dir.join(&name);
        std::fs::write(&abs, b"")?;
        let hash = blake3::hash(b"").to_hex().to_string();
        let md = std::fs::metadata(&abs)?;
        let rel = if folder.is_empty() {
            name.clone()
        } else {
            format!("{folder}/{name}")
        };
        let id = self.insert_new_asset(
            &rel,
            b"",
            &hash,
            md.len() as i64,
            crate::mtime_secs(&md),
            "create",
            "新建",
        )?;
        self.asset(&id)
    }
}
