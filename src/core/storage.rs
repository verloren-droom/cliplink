use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::{
    constants::crypto::HISTORY_PURPOSE,
    core::{
        at_rest::LocalDataCipher,
        error::{AppError, AppResult},
        model::ClipboardItem,
        paths::AppPaths,
    },
};

pub struct HistoryStore {
    conn: Mutex<Connection>,
    cipher: LocalDataCipher,
}

impl HistoryStore {
    pub fn open(paths: &AppPaths, cipher: LocalDataCipher) -> AppResult<Self> {
        let mut conn = Connection::open(&paths.history_db)?;
        configure_connection(&conn)?;
        migrate_schema_if_needed(&mut conn, &cipher)?;
        Ok(Self {
            conn: Mutex::new(conn),
            cipher,
        })
    }

    pub fn insert(&self, item: &ClipboardItem, limit: usize) -> AppResult<bool> {
        let conn = self.conn.lock();
        let latest_signature: Option<String> = conn
            .query_row(
                "SELECT signature FROM history ORDER BY created_at DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .ok();
        if latest_signature.as_deref() == Some(item.signature.as_str()) {
            return Ok(false);
        }

        conn.execute(
            "INSERT INTO history (id, signature, created_at, is_pinned, payload_blob)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                item.id.to_string(),
                item.signature.clone(),
                created_at_sort_key(item),
                if item.is_pinned { 1_i64 } else { 0_i64 },
                self.encrypt_item(item)?
            ],
        )?;

        drop(conn);
        self.enforce_limit(limit)?;
        Ok(true)
    }

    pub fn enforce_limit(&self, limit: usize) -> AppResult<()> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, is_pinned
             FROM history
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? != 0))
        })?;

        let mut unlocked_kept = 0usize;
        let mut delete_ids = Vec::new();
        for row in rows {
            let (id, is_pinned) = row?;
            if is_pinned {
                continue;
            }

            if unlocked_kept < limit.max(1) {
                unlocked_kept += 1;
            } else {
                delete_ids.push(id);
            }
        }
        drop(stmt);

        for id in delete_ids {
            conn.execute("DELETE FROM history WHERE id = ?1", params![id])?;
        }
        Ok(())
    }

    pub fn recent(&self, limit: usize) -> AppResult<Vec<ClipboardItem>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT payload_blob, is_pinned
             FROM history
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)? != 0))
        })?;
        let mut items = Vec::new();
        let mut unlocked_kept = 0usize;
        for row in rows {
            let (payload_blob, is_pinned) = row?;
            if !is_pinned && unlocked_kept >= limit.max(1) {
                continue;
            }

            let mut item = self.decrypt_item(&payload_blob)?;
            item.is_pinned = is_pinned;
            if !is_pinned {
                unlocked_kept += 1;
            }
            items.push(item);
        }
        Ok(items)
    }

    pub fn find_by_id(&self, id: Uuid) -> AppResult<Option<ClipboardItem>> {
        let payload_blob: Option<(Vec<u8>, bool)> = self
            .conn
            .lock()
            .query_row(
                "SELECT payload_blob, is_pinned FROM history WHERE id = ?1 LIMIT 1",
                params![id.to_string()],
                |row| Ok((row.get(0)?, row.get::<_, i64>(1)? != 0)),
            )
            .optional()?;

        payload_blob
            .map(|(value, is_pinned)| {
                let mut item = self.decrypt_item(&value)?;
                item.is_pinned = is_pinned;
                Ok(item)
            })
            .transpose()
    }

    pub fn update_item(&self, item: &ClipboardItem) -> AppResult<()> {
        self.conn.lock().execute(
            "UPDATE history
             SET created_at = ?2,
                 is_pinned = ?3,
                 payload_blob = ?4
             WHERE id = ?1",
            params![
                item.id.to_string(),
                created_at_sort_key(item),
                if item.is_pinned { 1_i64 } else { 0_i64 },
                self.encrypt_item(item)?
            ],
        )?;
        Ok(())
    }

    pub fn clear_unpinned(&self) -> AppResult<usize> {
        let affected = self
            .conn
            .lock()
            .execute("DELETE FROM history WHERE is_pinned = 0", [])?;
        Ok(affected)
    }

    pub fn delete_by_id(&self, id: Uuid) -> AppResult<bool> {
        let affected = self
            .conn
            .lock()
            .execute("DELETE FROM history WHERE id = ?1", params![id.to_string()])?;
        Ok(affected > 0)
    }

    fn encrypt_item(&self, item: &ClipboardItem) -> AppResult<Vec<u8>> {
        let json = serde_json::to_vec(item)?;
        self.cipher.seal(HISTORY_PURPOSE, &json)
    }

    fn decrypt_item(&self, payload_blob: &[u8]) -> AppResult<ClipboardItem> {
        let json = self.cipher.open(HISTORY_PURPOSE, payload_blob)?;
        serde_json::from_slice(&json).map_err(Into::into)
    }
}

fn configure_connection(conn: &Connection) -> AppResult<()> {
    #[cfg(target_os = "android")]
    conn.execute_batch(
        "PRAGMA cache_size=-256;
         PRAGMA mmap_size=0;
         PRAGMA journal_mode=WAL;
         PRAGMA synchronous=FULL;",
    )?;

    #[cfg(not(target_os = "android"))]
    conn.execute_batch(
        "PRAGMA cache_size=-512;
         PRAGMA mmap_size=0;
         PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;",
    )?;
    Ok(())
}

fn migrate_schema_if_needed(conn: &mut Connection, cipher: &LocalDataCipher) -> AppResult<()> {
    let history_exists = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'history' LIMIT 1",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();

    if !history_exists {
        create_history_schema(conn)?;
        return Ok(());
    }

    let (has_payload_blob, has_payload_json, has_is_pinned) = {
        let mut stmt = conn.prepare("PRAGMA table_info(history)")?;
        let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
        let mut has_payload_blob = false;
        let mut has_payload_json = false;
        let mut has_is_pinned = false;
        for column in columns {
            match column?.as_str() {
                "payload_blob" => has_payload_blob = true,
                "payload_json" => has_payload_json = true,
                "is_pinned" => has_is_pinned = true,
                _ => {}
            }
        }
        (has_payload_blob, has_payload_json, has_is_pinned)
    };

    if has_payload_blob && !has_payload_json {
        if !has_is_pinned {
            conn.execute(
                "ALTER TABLE history ADD COLUMN is_pinned INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        create_history_index(conn)?;
        return Ok(());
    }

    if has_payload_json {
        migrate_legacy_payload_json(conn, cipher)?;
        return Ok(());
    }

    Err(AppError::Sql(rusqlite::Error::InvalidColumnName(
        "history.payload_blob".to_string(),
    )))
}

fn create_history_schema(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS history (
           id TEXT PRIMARY KEY,
           signature TEXT NOT NULL,
           created_at INTEGER NOT NULL,
           is_pinned INTEGER NOT NULL DEFAULT 0,
           payload_blob BLOB NOT NULL
         );",
    )?;
    create_history_index(conn)
}

fn create_history_index(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "CREATE INDEX IF NOT EXISTS history_created_at_idx ON history(created_at DESC)",
        [],
    )?;
    Ok(())
}

fn created_at_sort_key(item: &ClipboardItem) -> i64 {
    item.created_at.unix_timestamp_nanos() as i64
}

fn migrate_legacy_payload_json(conn: &mut Connection, cipher: &LocalDataCipher) -> AppResult<()> {
    let transaction = conn.transaction()?;
    transaction.execute_batch(
        "DROP TABLE IF EXISTS history_v2;
         CREATE TABLE history_v2 (
           id TEXT PRIMARY KEY,
           signature TEXT NOT NULL,
           created_at INTEGER NOT NULL,
           is_pinned INTEGER NOT NULL DEFAULT 0,
           payload_blob BLOB NOT NULL
         );",
    )?;

    {
        let mut select = transaction.prepare(
            "SELECT id, signature, created_at, payload_json
             FROM history
             ORDER BY created_at DESC",
        )?;
        let mut rows = select.query([])?;
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let signature: String = row.get(1)?;
            let created_at: i64 = row.get(2)?;
            let payload_json: String = row.get(3)?;
            let payload_blob = cipher.seal(HISTORY_PURPOSE, payload_json.as_bytes())?;
            transaction.execute(
                "INSERT INTO history_v2 (id, signature, created_at, is_pinned, payload_blob)
                 VALUES (?1, ?2, ?3, 0, ?4)",
                params![id, signature, created_at, payload_blob],
            )?;
        }
    }

    transaction.execute_batch(
        "DROP INDEX IF EXISTS history_created_at_idx;
         DROP TABLE history;
         ALTER TABLE history_v2 RENAME TO history;",
    )?;
    create_history_index(&transaction)?;
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rusqlite::{Connection, params};
    use time::{Duration, OffsetDateTime};
    use uuid::Uuid;

    use super::HistoryStore;
    use crate::core::{
        at_rest::LocalDataCipher,
        model::{ClipboardItem, ClipboardKind, ClipboardPayload},
        paths::AppPaths,
    };

    fn test_paths(name: &str) -> AppPaths {
        let root =
            std::env::temp_dir().join(format!("cliplink-storage-test-{name}-{}", Uuid::new_v4()));
        let paths = AppPaths::from_root(root);
        paths.ensure().unwrap();
        paths
    }

    fn test_cipher() -> LocalDataCipher {
        LocalDataCipher::from_bytes([5_u8; 32])
    }

    fn sample_text_item(text: &str) -> ClipboardItem {
        ClipboardItem {
            id: Uuid::new_v4(),
            kind: ClipboardKind::Text,
            summary: text.to_string(),
            signature: format!("sig-{text}"),
            payload: ClipboardPayload::Text(text.to_string()),
            source_device_id: Some("local-device".to_string()),
            source_device_name: Some("Test Device".to_string()),
            created_at: OffsetDateTime::now_utc(),
            is_remote: false,
            is_pinned: false,
        }
    }

    #[test]
    fn encrypted_history_roundtrip_survives_reopen() {
        let paths = test_paths("roundtrip");
        let store = HistoryStore::open(&paths, test_cipher()).unwrap();
        let item = sample_text_item("hello encrypted history");
        store.insert(&item, 10).unwrap();
        drop(store);

        let reopened = HistoryStore::open(&paths, test_cipher()).unwrap();
        let recent = reopened.recent(10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].payload, item.payload);

        fs::remove_dir_all(paths.root).unwrap();
    }

    #[test]
    fn legacy_plaintext_history_is_migrated() {
        let paths = test_paths("legacy");
        let connection = Connection::open(&paths.history_db).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE history (
                   id TEXT PRIMARY KEY,
                   signature TEXT NOT NULL,
                   created_at INTEGER NOT NULL,
                   is_pinned INTEGER NOT NULL DEFAULT 0,
                   payload_json TEXT NOT NULL
                 );
                 CREATE INDEX history_created_at_idx ON history(created_at DESC);",
            )
            .unwrap();

        let item = sample_text_item("legacy plaintext item");
        connection
            .execute(
                "INSERT INTO history (id, signature, created_at, is_pinned, payload_json)
                 VALUES (?1, ?2, ?3, 0, ?4)",
                params![
                    item.id.to_string(),
                    item.signature.clone(),
                    item.created_at.unix_timestamp_nanos() as i64,
                    serde_json::to_string(&item).unwrap()
                ],
            )
            .unwrap();
        drop(connection);

        let migrated = HistoryStore::open(&paths, test_cipher()).unwrap();
        let recent = migrated.recent(10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].payload, item.payload);

        let verify = Connection::open(&paths.history_db).unwrap();
        let payload_blob_columns: i64 = verify
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('history') WHERE name = 'payload_blob'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let payload_json_columns: i64 = verify
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('history') WHERE name = 'payload_json'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(payload_blob_columns, 1);
        assert_eq!(payload_json_columns, 0);

        fs::remove_dir_all(paths.root).unwrap();
    }

    #[test]
    fn pinned_items_survive_limit_enforcement_and_clear() {
        let paths = test_paths("pinned");
        let store = HistoryStore::open(&paths, test_cipher()).unwrap();

        let mut pinned = sample_text_item("pinned");
        pinned.is_pinned = true;
        pinned.created_at -= Duration::seconds(2);
        store.insert(&pinned, 1).unwrap();

        let mut regular_one = sample_text_item("regular-1");
        regular_one.created_at -= Duration::seconds(1);
        store.insert(&regular_one, 1).unwrap();

        let regular_two = sample_text_item("regular-2");
        store.insert(&regular_two, 1).unwrap();

        let recent = store.recent(1).unwrap();
        assert_eq!(recent.len(), 2);
        assert!(
            recent
                .iter()
                .any(|item| item.signature == pinned.signature && item.is_pinned)
        );
        assert!(
            recent
                .iter()
                .any(|item| item.signature == regular_two.signature)
        );

        let deleted = store.clear_unpinned().unwrap();
        assert_eq!(deleted, 1);

        let remaining = store.recent(10).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].signature, pinned.signature);
        assert!(remaining[0].is_pinned);

        fs::remove_dir_all(paths.root).unwrap();
    }
}
