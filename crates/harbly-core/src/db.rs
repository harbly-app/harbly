use crate::error::Result;
use rusqlite::Connection;

pub const SCHEMA_VERSION: i64 = 1;

pub fn open(path: &std::path::Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    // The MCP server process and the app share this database; WAL plus a busy
    // timeout make the occasional concurrent write wait instead of failing.
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
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
            updated_at   INTEGER NOT NULL,
            favorite     INTEGER NOT NULL DEFAULT 0
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

        CREATE TABLE IF NOT EXISTS ai_runs(
            id          TEXT PRIMARY KEY,
            asset_id    TEXT NOT NULL,
            kind        TEXT NOT NULL,
            supply      TEXT NOT NULL,
            model       TEXT NOT NULL DEFAULT '',
            instruction TEXT NOT NULL,
            status      TEXT NOT NULL,
            ver         INTEGER,
            report      TEXT,
            error       TEXT,
            created_at  INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_ai_runs_asset ON ai_runs(asset_id, created_at);

        CREATE TABLE IF NOT EXISTS ai_sessions(
            id               TEXT PRIMARY KEY,
            title            TEXT NOT NULL DEFAULT '',
            supply           TEXT NOT NULL,
            model            TEXT NOT NULL DEFAULT '',
            effort           TEXT NOT NULL DEFAULT '',
            agent_session_id TEXT,
            created_at       INTEGER NOT NULL,
            updated_at       INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS ai_messages(
            id         TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            role       TEXT NOT NULL,
            content    TEXT NOT NULL,
            actions    TEXT NOT NULL DEFAULT '[]',
            created_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_ai_messages_session ON ai_messages(session_id, created_at);
        "#,
    )?;
    // Columns added after the table first shipped; the error on re-run
    // ("duplicate column") is the expected steady state.
    let _ = conn.execute("ALTER TABLE ai_runs ADD COLUMN session_id TEXT", []);
    let _ = conn.execute("ALTER TABLE ai_runs ADD COLUMN message_id TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE assets ADD COLUMN favorite INTEGER NOT NULL DEFAULT 0",
        [],
    );
    conn.execute(
        "INSERT OR REPLACE INTO meta(key, value) VALUES('schema_version', ?1)",
        [SCHEMA_VERSION.to_string()],
    )?;
    Ok(())
}
