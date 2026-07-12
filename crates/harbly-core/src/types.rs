use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetMeta {
    pub id: String,
    /// Path relative to the library root, always '/'-separated
    pub rel_path: String,
    pub file_name: String,
    /// Containing folder (relative path; empty string for the root)
    pub folder: String,
    pub title: String,
    pub source: String,
    pub size_bytes: i64,
    pub current_hash: String,
    pub ver_count: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub tags: Vec<String>,
    /// Starred by the user; mirrors the on-file com.harbly.favorite xattr
    pub favorite: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagInfo {
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    pub name: String,
    pub rel: String,
    /// Recursive asset count
    pub count: i64,
    pub children: Vec<TreeNode>,
    /// HTML files directly inside this folder (shown when the sidebar tree is expanded)
    pub files: Vec<TreeFile>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeFile {
    pub id: String,
    pub name: String,
    pub ver_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub ver: i64,
    pub hash: String,
    pub label: String,
    pub size_bytes: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub asset: AssetMeta,
    pub snippet: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub found: usize,
    pub indexed: usize,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub total: usize,
    pub added: usize,
    pub updated: usize,
    pub moved: usize,
    pub removed: usize,
    /// Number of files whose on-disk xattr tags disagreed with the database and were adopted
    pub tags_synced: usize,
}

impl ScanSummary {
    /// Whether this scan actually changed the index — the watcher uses this to decide whether to notify the frontend to refresh
    pub fn changed(&self) -> bool {
        self.added + self.updated + self.moved + self.removed + self.tags_synced > 0
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub added: usize,
    pub duplicates: usize,
    pub renamed: usize,
    pub skipped: usize,
    /// Ids of the existing in-library assets the duplicates matched (for the frontend's "view existing" action)
    pub dup_of: Vec<String>,
    /// Relative paths actually written to disk this time (recorded for the undo log)
    pub imported: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortKey {
    /// Recently added
    Recent,
    /// Name
    Name,
    /// Date modified
    Modified,
}
