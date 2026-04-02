use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::{
    constants::crypto::HISTORY_PURPOSE,
    core::{at_rest::LocalDataCipher, error::AppResult, model::ClipboardItem, paths::AppPaths},
};

pub struct HistoryStore {
    conn: Mutex<Connection>,
    cipher: LocalDataCipher,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryStoreProfile {
    pub cache_size_kib: usize,
    pub full_sync: bool,
}

impl Default for HistoryStoreProfile {
    fn default() -> Self {
        Self {
            cache_size_kib: 256,
            full_sync: false,
        }
    }
}

pub struct InsertResult {
    pub inserted: bool,
    pub pruned_ids: Vec<Uuid>,
}

impl HistoryStore {
    pub fn open(
        paths: &AppPaths,
        cipher: LocalDataCipher,
        profile: HistoryStoreProfile,
    ) -> AppResult<Self> {
        let mut conn = Connection::open(&paths.history_db)?;
        configure_connection(&conn, profile)?;
        migrate_schema_if_needed(&mut conn)?;
        let store = Self {
            conn: Mutex::new(conn),
            cipher,
        };
        store.reset_on_corrupted_payloads()?;
        Ok(store)
    }

    pub fn insert(&self, item: &ClipboardItem, limit: usize) -> AppResult<InsertResult> {
        let conn = self.conn.lock();
        let latest_item = conn
            .query_row(
                "SELECT payload_blob, is_pinned
                 FROM history
                 ORDER BY created_at DESC
                 LIMIT 1",
                [],
                |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)? != 0)),
            )
            .optional()?;
        if let Some((payload_blob, is_pinned)) = latest_item {
            let mut latest_item = self.decrypt_item(&payload_blob)?;
            latest_item.is_pinned = is_pinned;
            if latest_item.id != item.id
                && latest_item.signature == item.signature
                && latest_item.source_device_id == item.source_device_id
                && latest_item.source_device_name == item.source_device_name
                && latest_item.is_remote == item.is_remote
            {
                return Ok(InsertResult {
                    inserted: false,
                    pruned_ids: Vec::new(),
                });
            }
        }

        conn.execute(
            "INSERT INTO history (id, signature, created_at, is_pinned, payload_blob)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
                 signature = excluded.signature,
                 created_at = excluded.created_at,
                 is_pinned = MAX(history.is_pinned, excluded.is_pinned),
                 payload_blob = excluded.payload_blob",
            params![
                item.id.to_string(),
                item.signature.clone(),
                created_at_sort_key(item),
                if item.is_pinned { 1_i64 } else { 0_i64 },
                self.encrypt_item(item)?
            ],
        )?;

        drop(conn);
        let pruned_ids = self.enforce_limit(limit)?;
        Ok(InsertResult {
            inserted: true,
            pruned_ids,
        })
    }

    pub fn enforce_limit(&self, limit: usize) -> AppResult<Vec<Uuid>> {
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

        let mut removed_ids = Vec::with_capacity(delete_ids.len());
        for id in delete_ids {
            conn.execute("DELETE FROM history WHERE id = ?1", params![id])?;
            if let Ok(parsed) = Uuid::parse_str(&id) {
                removed_ids.push(parsed);
            }
        }
        Ok(removed_ids)
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

    #[cfg(test)]
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

    fn reset_on_corrupted_payloads(&self) -> AppResult<()> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT payload_blob
             FROM history
             ORDER BY created_at DESC
             LIMIT 1",
        )?;
        let payload_blob = stmt
            .query_row([], |row| row.get::<_, Vec<u8>>(0))
            .optional()?;
        drop(stmt);

        let Some(payload_blob) = payload_blob else {
            return Ok(());
        };

        if self.decrypt_item(&payload_blob).is_ok() {
            return Ok(());
        }

        // Legacy / corrupted rows are not supported; clear once to recover runtime.
        conn.execute("DELETE FROM history", [])?;
        Ok(())
    }
}

fn configure_connection(conn: &Connection, profile: HistoryStoreProfile) -> AppResult<()> {
    let sync_mode = if profile.full_sync { "FULL" } else { "NORMAL" };
    conn.execute_batch(&format!(
        "PRAGMA cache_size=-{};
         PRAGMA mmap_size=0;
         PRAGMA journal_mode=WAL;
         PRAGMA synchronous={sync_mode};",
        profile.cache_size_kib.max(1)
    ))?;
    Ok(())
}

fn migrate_schema_if_needed(conn: &mut Connection) -> AppResult<()> {
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

    let (has_payload_blob, has_is_pinned) = {
        let mut stmt = conn.prepare("PRAGMA table_info(history)")?;
        let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
        let mut has_payload_blob = false;
        let mut has_is_pinned = false;
        for column in columns {
            match column?.as_str() {
                "payload_blob" => has_payload_blob = true,
                "is_pinned" => has_is_pinned = true,
                _ => {}
            }
        }
        (has_payload_blob, has_is_pinned)
    };

    if has_payload_blob {
        if !has_is_pinned {
            conn.execute(
                "ALTER TABLE history ADD COLUMN is_pinned INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        create_history_index(conn)?;
        return Ok(());
    }

    // Old plaintext/legacy schema is intentionally not supported.
    // Drop and recreate a clean encrypted schema to keep startup stable.
    conn.execute_batch(
        "DROP TABLE IF EXISTS history;
         DROP INDEX IF EXISTS history_created_at_idx;",
    )?;
    create_history_schema(conn)
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

#[cfg(test)]
mod tests {
    use std::fs;

    use time::{Duration, OffsetDateTime};
    use uuid::Uuid;

    use super::{HistoryStore, HistoryStoreProfile};
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

    fn sample_remote_text_item(text: &str, device_id: &str, device_name: &str) -> ClipboardItem {
        ClipboardItem {
            id: Uuid::new_v4(),
            kind: ClipboardKind::Text,
            summary: text.to_string(),
            signature: format!("sig-{text}"),
            payload: ClipboardPayload::Text(text.to_string()),
            source_device_id: Some(device_id.to_string()),
            source_device_name: Some(device_name.to_string()),
            created_at: OffsetDateTime::now_utc(),
            is_remote: true,
            is_pinned: false,
        }
    }

    #[test]
    fn encrypted_history_roundtrip_survives_reopen() {
        let paths = test_paths("roundtrip");
        let store =
            HistoryStore::open(&paths, test_cipher(), HistoryStoreProfile::default()).unwrap();
        let item = sample_text_item("hello encrypted history");
        store.insert(&item, 10).unwrap();
        drop(store);

        let reopened =
            HistoryStore::open(&paths, test_cipher(), HistoryStoreProfile::default()).unwrap();
        let recent = reopened.recent(10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].payload, item.payload);

        fs::remove_dir_all(paths.root).unwrap();
    }

    #[test]
    fn same_text_from_different_sources_is_preserved() {
        let paths = test_paths("different-sources");
        let store =
            HistoryStore::open(&paths, test_cipher(), HistoryStoreProfile::default()).unwrap();
        let local = sample_text_item("shared text");
        let mut remote = sample_remote_text_item("shared text", "remote-device", "Remote Device");
        remote.created_at = local.created_at + Duration::seconds(1);

        assert!(store.insert(&local, 10).unwrap().inserted);
        assert!(store.insert(&remote, 10).unwrap().inserted);

        let recent = store.recent(10).unwrap();
        assert_eq!(recent.len(), 2);
        assert!(recent[0].is_remote);
        assert!(!recent[1].is_remote);

        fs::remove_dir_all(paths.root).unwrap();
    }

    #[test]
    fn reinserting_same_id_updates_existing_record() {
        let paths = test_paths("same-id-update");
        let store =
            HistoryStore::open(&paths, test_cipher(), HistoryStoreProfile::default()).unwrap();
        let mut item = sample_remote_text_item("hello", "remote-device", "Remote Device");

        assert!(store.insert(&item, 10).unwrap().inserted);
        item.created_at += Duration::seconds(5);
        item.payload = ClipboardPayload::Text("updated".to_string());
        item.summary = "updated".to_string();
        item.signature = "sig-updated".to_string();
        assert!(store.insert(&item, 10).unwrap().inserted);

        let recent = store.recent(10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].id, item.id);
        assert_eq!(recent[0].payload, item.payload);
        assert_eq!(recent[0].signature, item.signature);

        fs::remove_dir_all(paths.root).unwrap();
    }

    #[test]
    fn pinned_items_survive_limit_enforcement_and_clear() {
        let paths = test_paths("pinned");
        let store =
            HistoryStore::open(&paths, test_cipher(), HistoryStoreProfile::default()).unwrap();

        let mut pinned = sample_text_item("pinned");
        pinned.is_pinned = true;
        pinned.created_at -= Duration::seconds(2);
        let pinned_insert = store.insert(&pinned, 1).unwrap();
        assert!(pinned_insert.pruned_ids.is_empty());

        let mut regular_one = sample_text_item("regular-1");
        regular_one.created_at -= Duration::seconds(1);
        let first_regular_insert = store.insert(&regular_one, 1).unwrap();
        assert!(first_regular_insert.pruned_ids.is_empty());

        let regular_two = sample_text_item("regular-2");
        let second_regular_insert = store.insert(&regular_two, 1).unwrap();
        assert_eq!(second_regular_insert.pruned_ids, vec![regular_one.id]);

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
