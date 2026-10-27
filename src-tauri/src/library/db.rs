use crate::library::model::LibraryLoadResult;
use crate::library::now_ms;
use crate::parser::model::{strip_match_rounds, Account, MatchRecord, SnapshotAchievement};
use rusqlite::{Connection, OptionalExtension, Result};
use std::path::PathBuf;
use std::time::Duration;

pub const CURRENT_SCHEMA_VERSION: i64 = 7;
const LIBRARY_BUSY_TIMEOUT: Duration = Duration::from_secs(5);
static MIGRATION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn library_dir() -> std::result::Result<PathBuf, String> {
    let base = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("USERPROFILE").map(|home| format!("{home}\\AppData\\Local")))
        .map_err(|_| "LOCALAPPDATA and USERPROFILE are not set".to_string())?;
    Ok(PathBuf::from(base).join("wonderful-ui"))
}

pub fn open_library() -> std::result::Result<Connection, String> {
    let dir = library_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {}", dir.display(), e))?;
    let path = dir.join("library.db");
    let conn = Connection::open(&path).map_err(|e| format!("open {}: {}", path.display(), e))?;
    configure_library_connection(&conn, &path)?;
    Ok(conn)
}

fn configure_library_connection(
    conn: &Connection,
    path: &std::path::Path,
) -> std::result::Result<(), String> {
    // Every command opens its own connection. Migrations, scrape transactions,
    // preferences, and legacy asset writes can briefly overlap; wait for a
    // normal writer handoff instead of failing immediately with SQLITE_BUSY.
    conn.busy_timeout(LIBRARY_BUSY_TIMEOUT)
        .map_err(|e| format!("busy timeout {}: {}", path.display(), e))?;
    let version = stored_schema_version(conn).map_err(|e| e.to_string())?;
    ensure_supported_schema(version).map_err(|e| e.to_string())?;
    let journal: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if !journal.eq_ignore_ascii_case("wal") && journal != "memory" {
        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .map_err(|e| e.to_string())?;
    }
    conn.execute_batch("PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;")
        .map_err(|e| format!("pragma {}: {}", path.display(), e))?;
    if !schema_is_current(conn).map_err(|e| e.to_string())? {
        let _guard = MIGRATION_LOCK
            .lock()
            .map_err(|_| "资料库初始化锁失败".to_string())?;
        if !schema_is_current(conn).map_err(|e| e.to_string())? {
            migrate(conn).map_err(|e| format!("migrate {}: {}", path.display(), e))?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub fn open_memory_for_test() -> Result<Connection> {
    Connection::open_in_memory()
}

fn stored_schema_version(conn: &Connection) -> Result<Option<i64>> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='meta')",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(None);
    }
    let version: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    version
        .map(|v| {
            v.parse().map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .transpose()
}

fn ensure_supported_schema(version: Option<i64>) -> Result<()> {
    if version.is_some_and(|version| version > CURRENT_SCHEMA_VERSION) {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
            Some("资料库由较新版本创建，请更新软件后再打开".into()),
        ));
    }
    Ok(())
}

fn schema_is_current(conn: &Connection) -> Result<bool> {
    let version = stored_schema_version(conn)?;
    ensure_supported_schema(version)?;
    if version != Some(CURRENT_SCHEMA_VERSION) {
        return Ok(false);
    }
    let tables: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('meta','sources','scrape_jobs','accounts','account_preferences','snapshot_achievements','matches','videos','events','assets','history_devices','history_accounts','history_matches','history_videos')", [], |row| row.get(0))?;
    if tables != 14 {
        return Ok(false);
    }
    for column in [
        "parse_error",
        "parse_error_code",
        "match_details_json",
        "source_size_bytes",
        "source_mtime_ms",
        "snapshot_size_bytes",
        "snapshot_mtime_ms",
    ] {
        if !column_exists(conn, "accounts", column)? {
            return Ok(false);
        }
    }
    Ok(column_exists(conn, "scrape_jobs", "errors_seen")?
        && column_exists(conn, "scrape_jobs", "skipped_accounts")?)
}

pub fn migrate(conn: &Connection) -> Result<()> {
    ensure_supported_schema(stored_schema_version(conn)?)?;
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS history_devices (
            id TEXT PRIMARY KEY, label TEXT NOT NULL, source_bytes INTEGER NOT NULL,
            created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS history_accounts (
            device_id TEXT NOT NULL REFERENCES history_devices(id), openid TEXT NOT NULL,
            raw_json TEXT NOT NULL, PRIMARY KEY(device_id,openid)
        );
        CREATE TABLE IF NOT EXISTS history_matches (
            device_id TEXT NOT NULL REFERENCES history_devices(id), openid TEXT NOT NULL,
            match_id TEXT NOT NULL, raw_json TEXT NOT NULL,
            PRIMARY KEY(device_id,openid,match_id)
        );
        CREATE TABLE IF NOT EXISTS history_videos (
            device_id TEXT NOT NULL REFERENCES history_devices(id), openid TEXT NOT NULL,
            match_id TEXT NOT NULL, path TEXT NOT NULL,
            PRIMARY KEY(device_id,openid,match_id,path)
        );
        CREATE INDEX IF NOT EXISTS idx_history_match_account ON history_matches(openid,match_id);
        CREATE INDEX IF NOT EXISTS idx_history_video_path ON history_videos(path);

        CREATE TABLE IF NOT EXISTS meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS sources (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            root_path TEXT,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS scrape_jobs (
            id TEXT PRIMARY KEY,
            source_id TEXT,
            trigger TEXT NOT NULL,
            started_at INTEGER NOT NULL,
            finished_at INTEGER,
            status TEXT NOT NULL,
            message TEXT,
            matches_seen INTEGER NOT NULL DEFAULT 0,
            videos_seen INTEGER NOT NULL DEFAULT 0,
            events_seen INTEGER NOT NULL DEFAULT 0,
            skipped_accounts INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS accounts (
            openid TEXT PRIMARY KEY,
            nick TEXT,
            tag TEXT,
            parse_error TEXT,
            source_id TEXT NOT NULL,
            source_path TEXT,
            source_size_bytes INTEGER,
            source_mtime_ms INTEGER,
            snapshot_size_bytes INTEGER,
            snapshot_mtime_ms INTEGER,
            last_seen_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS snapshot_achievements (
            openid TEXT NOT NULL,
            matches_id TEXT NOT NULL,
            achv_type TEXT NOT NULL,
            type_str TEXT NOT NULL,
            source_id TEXT NOT NULL,
            last_seen_at INTEGER NOT NULL,
            PRIMARY KEY(openid, matches_id)
        );

        CREATE TABLE IF NOT EXISTS account_preferences (
            openid TEXT PRIMARY KEY,
            custom_name TEXT,
            sort_order INTEGER,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS matches (
            id TEXT PRIMARY KEY,
            source_id TEXT NOT NULL,
            source_match_id TEXT NOT NULL,
            openid TEXT,
            matches_time INTEGER NOT NULL,
            game_start_time TEXT,
            game_end_time TEXT,
            map_id TEXT,
            map_name TEXT,
            agent_id TEXT,
            agent_name TEXT,
            mode TEXT,
            mode_name TEXT,
            stats_json TEXT NOT NULL,
            career_json TEXT,
            raw_json TEXT,
            raw_hash TEXT,
            last_seen_at INTEGER NOT NULL
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_matches_source_match
            ON matches(source_id, source_match_id);

        CREATE TABLE IF NOT EXISTS videos (
            id TEXT PRIMARY KEY,
            match_id TEXT NOT NULL,
            source_id TEXT NOT NULL,
            source_video_id TEXT NOT NULL,
            video_type TEXT,
            name TEXT,
            path TEXT,
            poster_path TEXT,
            duration_ms INTEGER NOT NULL DEFAULT 0,
            fps INTEGER NOT NULL DEFAULT 0,
            resolution TEXT,
            size_bytes INTEGER NOT NULL DEFAULT 0,
            mtime_ms INTEGER,
            quick_hash TEXT,
            full_hash TEXT,
            video_hash TEXT,
            cover_hash TEXT,
            exists_on_disk INTEGER NOT NULL DEFAULT 0,
            last_seen_at INTEGER NOT NULL
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_videos_source_video
            ON videos(source_id, source_video_id);

        CREATE TABLE IF NOT EXISTS events (
            id TEXT PRIMARY KEY,
            match_id TEXT NOT NULL,
            video_id TEXT NOT NULL,
            source_event_id TEXT,
            event_type TEXT NOT NULL,
            event_time TEXT,
            time_ms INTEGER NOT NULL,
            seek_ms INTEGER NOT NULL,
            playback_seek_ms INTEGER NOT NULL,
            round_idx INTEGER NOT NULL,
            player_name TEXT,
            killer_name TEXT,
            killed_name TEXT,
            agent_name TEXT,
            weapon_path TEXT,
            weapon_name TEXT,
            is_headshot INTEGER NOT NULL DEFAULT 0,
            assist_num INTEGER NOT NULL DEFAULT 0,
            confidence INTEGER NOT NULL DEFAULT 100,
            dedup_key TEXT NOT NULL,
            raw_json TEXT
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_events_match_dedup
            ON events(match_id, dedup_key);

        CREATE TABLE IF NOT EXISTS assets (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            source_url TEXT,
            source_path TEXT,
            cache_path TEXT,
            hash TEXT,
            last_seen_at INTEGER NOT NULL
        );
        "#,
    )?;
    if !column_exists(conn, "accounts", "parse_error")? {
        conn.execute("ALTER TABLE accounts ADD COLUMN parse_error TEXT", [])?;
    }
    if !column_exists(conn, "accounts", "parse_error_code")? {
        conn.execute("ALTER TABLE accounts ADD COLUMN parse_error_code TEXT", [])?;
    }
    if !column_exists(conn, "accounts", "match_details_json")? {
        conn.execute(
            "ALTER TABLE accounts ADD COLUMN match_details_json TEXT",
            [],
        )?;
    }
    if !column_exists(conn, "accounts", "source_size_bytes")? {
        conn.execute(
            "ALTER TABLE accounts ADD COLUMN source_size_bytes INTEGER",
            [],
        )?;
    }
    if !column_exists(conn, "accounts", "source_mtime_ms")? {
        conn.execute(
            "ALTER TABLE accounts ADD COLUMN source_mtime_ms INTEGER",
            [],
        )?;
    }
    if !column_exists(conn, "accounts", "snapshot_size_bytes")? {
        conn.execute(
            "ALTER TABLE accounts ADD COLUMN snapshot_size_bytes INTEGER",
            [],
        )?;
    }
    if !column_exists(conn, "accounts", "snapshot_mtime_ms")? {
        conn.execute(
            "ALTER TABLE accounts ADD COLUMN snapshot_mtime_ms INTEGER",
            [],
        )?;
    }
    if !column_exists(conn, "scrape_jobs", "skipped_accounts")? {
        conn.execute(
            "ALTER TABLE scrape_jobs ADD COLUMN skipped_accounts INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if !column_exists(conn, "scrape_jobs", "errors_seen")? {
        conn.execute(
            "ALTER TABLE scrape_jobs ADD COLUMN errors_seen INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    conn.execute(
        "INSERT INTO meta(key, value) VALUES('schema_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value WHERE meta.value <> excluded.value",
        [CURRENT_SCHEMA_VERSION.to_string()],
    )?;
    Ok(())
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn load_library_view(conn: &Connection, dir: impl Into<String>) -> Result<LibraryLoadResult> {
    let mut achievements_stmt = conn.prepare(
        "SELECT matches_id, achv_type, type_str
         FROM snapshot_achievements
         WHERE openid = ?1
         ORDER BY matches_id",
    )?;
    // Keep named empty accounts and failures; hide anonymous empty shells.
    let mut accounts_stmt = conn.prepare(
        "SELECT
            a.openid,
            COALESCE(a.source_path, ''),
            COUNT(m.id) AS match_count,
            a.nick,
            a.tag,
            a.parse_error,
            p.custom_name,
            a.parse_error_code,
            a.match_details_json
         FROM accounts a
         LEFT JOIN matches m ON m.openid = a.openid
         LEFT JOIN account_preferences p ON p.openid = a.openid
         GROUP BY a.openid, a.source_path, a.nick, a.tag, a.parse_error, a.parse_error_code, a.match_details_json, p.custom_name, p.sort_order
         HAVING COUNT(m.id) > 0
            OR (a.parse_error IS NOT NULL AND TRIM(a.parse_error) <> '')
            OR TRIM(COALESCE(a.nick, '')) <> ''
            OR TRIM(COALESCE(p.custom_name, '')) <> ''
         ORDER BY
            CASE WHEN p.sort_order IS NULL THEN 1 ELSE 0 END,
            p.sort_order,
            a.openid",
    )?;
    let mut accounts = accounts_stmt
        .query_map([], |row| {
            let openid: String = row.get(0)?;
            let achievements = achievements_stmt
                .query_map([openid.as_str()], |achv_row| {
                    Ok(SnapshotAchievement {
                        matches_id: achv_row.get(0)?,
                        achv_type: achv_row.get(1)?,
                        type_str: achv_row.get(2)?,
                    })
                })?
                .collect::<Result<Vec<_>>>()?;
            Ok(Account {
                openid,
                path: row.get(1)?,
                match_count: row.get::<_, i64>(2)? as usize,
                nick: row.get(3)?,
                tag: row.get(4)?,
                custom_name: row.get(6)?,
                achievements,
                match_details: row
                    .get::<_, Option<String>>(8)?
                    .and_then(|json| serde_json::from_str(&json).ok())
                    .unwrap_or_default(),
                error: row.get(5)?,
                error_code: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>>>()?;

    let mut match_stmt =
        conn.prepare("SELECT id, openid, raw_json FROM matches ORDER BY matches_time DESC")?;
    let mut matches = Vec::new();
    let mut corrupt_matches = 0i64;
    let rows = match_stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for row in rows {
        let (match_id, openid, raw) = row?;
        match serde_json::from_str::<MatchRecord>(&raw) {
            Ok(m) => matches.push(m),
            Err(error) => {
                corrupt_matches += 1;
                crate::app_log::write(
                    crate::app_log::LogLevel::Error,
                    "library",
                    format!(
                        "skipping corrupt match row id={match_id} openid={} error={error}",
                        openid.as_deref().unwrap_or("unknown"),
                    ),
                );
            }
        }
    }
    // Strip rounds once after deserialize (bulk list must stay rounds-free for IPC size).
    let corrupt_history = super::history::merge_view(conn, &mut accounts, &mut matches)?;
    strip_match_rounds(&mut matches);
    let account_errors: i64 = conn.query_row(
        "SELECT COUNT(*) FROM accounts WHERE parse_error IS NOT NULL AND parse_error <> ''",
        [],
        |row| row.get(0),
    )?;

    Ok(LibraryLoadResult {
        dir: dir.into(),
        accounts,
        matches,
        total_errors: (account_errors + corrupt_matches) as usize + corrupt_history,
    })
}

pub fn save_account_order(conn: &Connection, openids: &[String]) -> Result<()> {
    let now = now_ms();
    let transaction = conn.unchecked_transaction()?;
    for (idx, openid) in openids.iter().enumerate() {
        transaction.execute(
            "INSERT INTO account_preferences(openid, custom_name, sort_order, updated_at)
             VALUES(?1, NULL, ?2, ?3)
             ON CONFLICT(openid) DO UPDATE SET
               sort_order = excluded.sort_order,
               updated_at = excluded.updated_at",
            rusqlite::params![openid, idx as i64, now],
        )?;
    }
    transaction.commit()
}

pub fn set_account_custom_name(
    conn: &Connection,
    openid: &str,
    custom_name: Option<&str>,
) -> Result<()> {
    let now = now_ms();
    let trimmed = custom_name.map(str::trim).filter(|s| !s.is_empty());
    conn.execute(
        "INSERT INTO account_preferences(openid, custom_name, updated_at)
         VALUES(?1, ?2, ?3)
         ON CONFLICT(openid) DO UPDATE SET
           custom_name = excluded.custom_name,
           updated_at = excluded.updated_at",
        rusqlite::params![openid, trimmed, now],
    )?;
    Ok(())
}

pub fn upsert_asset(
    conn: &Connection,
    kind: &str,
    source_url: &str,
    cache_path: &str,
    hash: &str,
) -> Result<()> {
    let now = now_ms();
    let id = format!("{}:{}", kind, hash);
    conn.execute(
        "INSERT INTO assets(id, kind, source_url, cache_path, hash, last_seen_at)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET
           last_seen_at = excluded.last_seen_at",
        rusqlite::params![id, kind, source_url, cache_path, hash, now],
    )?;
    Ok(())
}

pub fn load_match_rounds(
    conn: &Connection,
    openid: &str,
    match_id: &str,
) -> std::result::Result<MatchRecord, String> {
    super::history::load_match(conn, openid, match_id)
}

/// IPC commands that open, decode, or share a local file must only accept
/// paths that came from the persisted ACLOS video index. This keeps an
/// injected WebView script from turning a path-taking command into arbitrary
/// file execution or exfiltration.
pub fn is_library_video_path(conn: &Connection, path: &str) -> Result<bool> {
    conn.query_row(
        "SELECT ?1 <> '' AND (EXISTS(SELECT 1 FROM videos WHERE path = ?1)
         OR EXISTS(SELECT 1 FROM history_videos WHERE path = ?1))",
        [path],
        |row| row.get(0),
    )
}

pub fn is_known_account(conn: &Connection, openid: &str) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM accounts WHERE openid = ?1)
         OR EXISTS(SELECT 1 FROM history_accounts WHERE openid = ?1)",
        [openid],
        |row| row.get(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_schema_connections_do_not_write_and_future_schema_is_preserved() {
        let conn = open_memory_for_test().unwrap();
        configure_library_connection(&conn, std::path::Path::new(":memory:")).unwrap();
        let before = conn.total_changes();
        configure_library_connection(&conn, std::path::Path::new(":memory:")).unwrap();
        assert_eq!(conn.total_changes(), before);
        conn.execute("UPDATE meta SET value='999' WHERE key='schema_version'", [])
            .unwrap();
        assert!(configure_library_connection(&conn, std::path::Path::new(":memory:")).is_err());
        assert!(migrate(&conn).is_err());
        assert_eq!(stored_schema_version(&conn).unwrap(), Some(999));
    }

    #[test]
    fn migration_retains_legacy_errors_and_named_empty_accounts() {
        let conn = open_memory_for_test().unwrap();
        migrate(&conn).unwrap();
        conn.execute("ALTER TABLE accounts DROP COLUMN parse_error_code", [])
            .unwrap();
        conn.execute("ALTER TABLE accounts DROP COLUMN match_details_json", [])
            .unwrap();
        conn.execute(
            "INSERT INTO accounts(openid, source_id, last_seen_at, nick, parse_error)
            VALUES('legacy', 'aclos_wonderfuldb', 1, NULL, 'old error'),
                  ('named', 'aclos_wonderfuldb', 1, 'Fixture', NULL),
                  ('anonymous', 'aclos_wonderfuldb', 1, NULL, NULL)",
            [],
        )
        .unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();
        let view = load_library_view(&conn, "fixture").unwrap();
        assert_eq!(view.accounts.len(), 2);
        assert_eq!(view.total_errors, 1);
        let legacy = view.accounts.iter().find(|a| a.openid == "legacy").unwrap();
        assert_eq!(legacy.error.as_deref(), Some("old error"));
        assert!(legacy.error_code.is_none());
        assert!(legacy.match_details.is_empty());
        assert!(view
            .accounts
            .iter()
            .any(|a| a.openid == "named" && a.error.is_none()));
    }

    #[test]
    fn migrate_creates_schema_version_and_core_tables() {
        let conn = open_memory_for_test().expect("memory db opens");
        migrate(&conn).expect("migration succeeds");

        let version: String = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .expect("schema version exists");
        assert_eq!(version, CURRENT_SCHEMA_VERSION.to_string());

        for table in [
            "sources",
            "scrape_jobs",
            "accounts",
            "account_preferences",
            "snapshot_achievements",
            "matches",
            "videos",
            "events",
            "assets",
        ] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .expect("table lookup works");
            assert_eq!(count, 1, "missing table {table}");
        }
    }

    #[test]
    fn configured_connections_wait_for_short_sqlite_writer_handoffs() {
        let conn = open_memory_for_test().expect("memory db opens");
        configure_library_connection(&conn, std::path::Path::new(":memory:"))
            .expect("connection config succeeds");

        let timeout_ms: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .expect("busy timeout reads");
        assert_eq!(timeout_ms, LIBRARY_BUSY_TIMEOUT.as_millis() as i64);
    }

    #[test]
    fn video_path_authorization_requires_an_exact_persisted_path() {
        let conn = open_memory_for_test().expect("memory db opens");
        migrate(&conn).expect("migration succeeds");
        conn.execute(
            "INSERT INTO videos(
                id, match_id, source_id, source_video_id, path,
                duration_ms, fps, size_bytes, exists_on_disk, last_seen_at
             ) VALUES('video-1', 'match-1', 'aclos_wonderfuldb', 'source-video-1',
                      'D:\\Highlights\\clip.mp4', 1000, 60, 1234, 1, 1)",
            [],
        )
        .expect("video inserted");

        assert!(is_library_video_path(&conn, "D:\\Highlights\\clip.mp4").unwrap());
        assert!(!is_library_video_path(&conn, "D:\\Highlights\\other.mp4").unwrap());
        assert!(!is_library_video_path(&conn, "D:\\Highlights\\clip.mp4.exe").unwrap());
    }

    #[test]
    fn load_library_view_counts_corrupt_match_rows_instead_of_silently_hiding_them() {
        let conn = open_memory_for_test().expect("memory db opens");
        migrate(&conn).expect("migration succeeds");
        conn.execute(
            "INSERT INTO accounts(openid, source_id, source_path, last_seen_at)
             VALUES('account-a', 'aclos_wonderfuldb', 'fixture', 1)",
            [],
        )
        .expect("account inserted");
        conn.execute(
            "INSERT INTO matches(
                id, source_id, source_match_id, openid, matches_time,
                stats_json, raw_json, last_seen_at
             ) VALUES(
                'broken-match', 'aclos_wonderfuldb', 'broken-match',
                'account-a', 1, '{}', 'not-json', 1
             )",
            [],
        )
        .expect("corrupt match inserted");

        let view = load_library_view(&conn, "fixture").expect("view still loads");

        assert!(view.matches.is_empty());
        assert_eq!(view.total_errors, 1);
    }

    #[test]
    fn load_library_view_returns_empty_result_for_new_database() {
        let conn = open_memory_for_test().expect("memory db opens");
        migrate(&conn).expect("migration succeeds");
        let view = load_library_view(&conn, "memory").expect("load succeeds");

        assert_eq!(view.dir, "memory");
        assert!(view.accounts.is_empty());
        assert!(view.matches.is_empty());
        assert_eq!(view.total_errors, 0);
    }

    #[test]
    fn account_preferences_override_label_and_order_library_view() {
        let conn = open_memory_for_test().expect("memory db opens");
        migrate(&conn).expect("migration succeeds");
        conn.execute(
            "INSERT INTO accounts(openid, nick, tag, source_id, source_path, last_seen_at)
             VALUES
             ('a', 'Alpha', '1111', 'aclos_wonderfuldb', 'a-path', 1),
             ('b', 'Beta', '2222', 'aclos_wonderfuldb', 'b-path', 1),
             ('c', 'Gamma', '3333', 'aclos_wonderfuldb', 'c-path', 1)",
            [],
        )
        .expect("accounts inserted");
        // Seed one match each to exercise account counts as well as preferences.
        for (id, openid) in [("m-a", "a"), ("m-b", "b"), ("m-c", "c")] {
            let raw = format!(
                r#"{{"openID":"{openid}","matches_id":"{id}","matches_time":1,"videos":[]}}"#
            );
            conn.execute(
                "INSERT INTO matches(id, source_id, source_match_id, openid, matches_time, stats_json, raw_json, last_seen_at)
                 VALUES(?1, 'aclos_wonderfuldb', ?1, ?2, 1, '{{}}', ?3, 1)",
                rusqlite::params![id, openid, raw],
            )
            .expect("match inserted");
        }

        set_account_custom_name(&conn, "b", Some("  主账号  ")).expect("rename saved");
        save_account_order(&conn, &["c".into(), "b".into()]).expect("order saved");

        let view = load_library_view(&conn, "memory").expect("load succeeds");

        assert_eq!(
            view.accounts
                .iter()
                .map(|a| a.openid.as_str())
                .collect::<Vec<_>>(),
            vec!["c", "b", "a"]
        );
        let renamed = view
            .accounts
            .iter()
            .find(|a| a.openid == "b")
            .expect("b exists");
        assert_eq!(renamed.custom_name.as_deref(), Some("主账号"));

        set_account_custom_name(&conn, "b", Some(" ")).expect("rename cleared");
        let view = load_library_view(&conn, "memory").expect("load succeeds");
        let renamed = view
            .accounts
            .iter()
            .find(|a| a.openid == "b")
            .expect("b exists");
        assert_eq!(renamed.custom_name, None);
    }

    #[test]
    fn account_order_rolls_back_if_any_row_fails() {
        let conn = open_memory_for_test().expect("memory db opens");
        migrate(&conn).expect("migration succeeds");
        conn.execute(
            "INSERT INTO account_preferences(openid, sort_order, updated_at)
             VALUES('a', 7, 1), ('b', 8, 1)",
            [],
        )
        .expect("preferences seeded");
        conn.execute_batch(
            "CREATE TRIGGER reject_b_order
             BEFORE UPDATE OF sort_order ON account_preferences
             WHEN OLD.openid = 'b'
             BEGIN
               SELECT RAISE(ABORT, 'injected order failure');
             END;",
        )
        .expect("failure trigger installed");

        let error =
            save_account_order(&conn, &["a".into(), "b".into()]).expect_err("second row must fail");
        assert!(error.to_string().contains("injected order failure"));

        let orders = conn
            .prepare("SELECT openid, sort_order FROM account_preferences ORDER BY openid")
            .expect("query prepared")
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .expect("query runs")
            .collect::<Result<Vec<_>>>()
            .expect("rows collected");
        assert_eq!(orders, vec![("a".into(), 7), ("b".into(), 8)]);
    }
}
