use crate::error::Result;
use rusqlite::Connection;

pub const SCHEMA_VERSION: i64 = 1;

pub fn open(path: &std::path::Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);

        CREATE TABLE IF NOT EXISTS assets(
            id           TEXT PRIMARY KEY,
            rel_path     TEXT NOT NULL UNIQUE,
            folder       TEXT NOT NULL,
            title        TEXT NOT NULL,
            source       TEXT NOT NULL DEFAULT 'import',
            current_hash TEXT NOT NULL,
            size_bytes   INTEGER NOT NULL,
            mtime        INTEGER NOT NULL,
            created_at   INTEGER NOT NULL,
            updated_at   INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_assets_hash   ON assets(current_hash);
        CREATE INDEX IF NOT EXISTS idx_assets_folder ON assets(folder);

        CREATE TABLE IF NOT EXISTS versions(
            asset_id   TEXT NOT NULL,
            ver        INTEGER NOT NULL,
            hash       TEXT NOT NULL,
            label      TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            PRIMARY KEY(asset_id, ver)
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS fts USING fts5(
            asset_id UNINDEXED,
            title,
            body,
            tokenize='unicode61'
        );

        CREATE TABLE IF NOT EXISTS asset_tags(
            asset_id TEXT NOT NULL,
            tag      TEXT NOT NULL,
            PRIMARY KEY(asset_id, tag)
        );
        CREATE INDEX IF NOT EXISTS idx_tags_tag ON asset_tags(tag);
        "#,
    )?;
    conn.execute(
        "INSERT OR REPLACE INTO meta(key, value) VALUES('schema_version', ?1)",
        [SCHEMA_VERSION.to_string()],
    )?;
    Ok(())
}
