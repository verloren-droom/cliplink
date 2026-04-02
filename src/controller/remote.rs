use std::collections::{HashMap, HashSet, VecDeque};

use time::OffsetDateTime;
use uuid::Uuid;

use super::*;
use crate::{
    constants::limits::{
        MAX_REMOTE_CONTENT_CACHE_BYTES, MAX_REMOTE_CONTENT_CACHE_ITEMS,
        MAX_REMOTE_HISTORY_TOOLTIP_CHARS, MAX_REMOTE_HISTORY_TOOLTIP_LINES,
        MAX_REMOTE_SESSION_ITEMS_PER_PEER, MAX_REMOTE_SESSION_ITEMS_TOTAL,
    },
    core::model::{ClipboardItem, ClipboardKind, ClipboardPayload},
};

#[derive(Debug, Default)]
pub(super) struct RemoteHistorySession {
    items: HashMap<Uuid, RemoteHistoryItem>,
    content_cache: RemoteContentCache,
}

#[derive(Debug, Clone)]
pub(super) struct RemoteHistoryItem {
    pub(super) source_device_id: Option<String>,
    pub(super) signature: String,
    pub(super) total_size_bytes: u64,
    pub(super) history_entry: HistoryEntry,
}

#[derive(Debug, Default)]
struct RemoteContentCache {
    items: HashMap<Uuid, CachedRemoteContent>,
    order: VecDeque<Uuid>,
    total_bytes: usize,
}

#[derive(Debug, Clone)]
struct CachedRemoteContent {
    item: ClipboardItem,
    estimated_bytes: usize,
}

impl RemoteHistorySession {
    pub(super) fn clear(&mut self) -> Vec<Uuid> {
        let removed_ids = self.items.keys().copied().collect::<Vec<_>>();
        self.items.clear();
        self.content_cache.clear();
        removed_ids
    }

    pub(super) fn item(&self, id: Uuid) -> Option<&RemoteHistoryItem> {
        self.items.get(&id)
    }

    pub(super) fn cached_item(&self, id: Uuid) -> Option<ClipboardItem> {
        self.content_cache.get(id)
    }

    pub(super) fn contains(&self, id: Uuid) -> bool {
        self.items.contains_key(&id)
    }

    pub(super) fn len(&self) -> usize {
        self.items.len()
    }

    pub(super) fn entries(&self) -> impl Iterator<Item = &HistoryEntry> {
        self.items.values().map(|item| &item.history_entry)
    }

    pub(super) fn latest_cached_item(&self) -> Option<ClipboardItem> {
        self.content_cache
            .items
            .values()
            .max_by(|left, right| left.item.created_at.cmp(&right.item.created_at))
            .map(|entry| entry.item.clone())
    }

    pub(super) fn retain_online_devices(
        &mut self,
        online_device_ids: &HashSet<String>,
    ) -> Vec<Uuid> {
        let stale_ids = self
            .items
            .values()
            .filter(|item| {
                item.source_device_id
                    .as_ref()
                    .is_some_and(|device_id| !online_device_ids.contains(device_id))
            })
            .map(|item| item.history_entry.id)
            .collect::<Vec<_>>();

        self.remove_items(stale_ids)
    }

    pub(super) fn replace_peer_items(
        &mut self,
        peer_device_id: &str,
        items: Vec<ClipboardItem>,
    ) -> Vec<Uuid> {
        let incoming_items = items
            .into_iter()
            .take(MAX_REMOTE_SESSION_ITEMS_PER_PEER)
            .collect::<Vec<_>>();
        let incoming_ids = incoming_items
            .iter()
            .map(|item| item.id)
            .collect::<HashSet<_>>();

        let stale_ids = self
            .items
            .values()
            .filter(|item| {
                item.source_device_id.as_deref() == Some(peer_device_id)
                    && !incoming_ids.contains(&item.history_entry.id)
            })
            .map(|item| item.history_entry.id)
            .collect::<Vec<_>>();
        let mut removed_ids = self.remove_items(stale_ids);

        for (index, item) in incoming_items.into_iter().enumerate() {
            let remote_item = RemoteHistoryItem::from_clipboard_item(&item);
            self.items.insert(remote_item.history_entry.id, remote_item);
            if index == 0 {
                self.content_cache.insert(item);
            }
        }

        removed_ids.extend(self.prune_peer_limit(peer_device_id));
        removed_ids.extend(self.prune_total_limit());
        removed_ids
    }

    pub(super) fn upsert_live_item(&mut self, item: ClipboardItem) -> Vec<Uuid> {
        let peer_device_id = item.source_device_id.clone();
        let remote_item = RemoteHistoryItem::from_clipboard_item(&item);
        self.content_cache.insert(item);
        self.items.insert(remote_item.history_entry.id, remote_item);
        let mut removed_ids = Vec::new();
        if let Some(peer_device_id) = peer_device_id.as_deref() {
            removed_ids.extend(self.prune_peer_limit(peer_device_id));
        }
        removed_ids.extend(self.prune_total_limit());
        removed_ids
    }

    pub(super) fn remove_peer_items(
        &mut self,
        peer_device_id: &str,
        item_ids: &[Uuid],
    ) -> Vec<Uuid> {
        let stale_ids = item_ids
            .iter()
            .copied()
            .filter(|item_id| {
                self.items
                    .get(item_id)
                    .and_then(|item| item.source_device_id.as_deref())
                    == Some(peer_device_id)
            })
            .collect::<Vec<_>>();
        self.remove_items(stale_ids)
    }

    pub(super) fn cache_item(&mut self, item: ClipboardItem) {
        self.content_cache.insert(item);
    }

    pub(super) fn touch_item(&mut self, id: Uuid, created_at: OffsetDateTime) -> bool {
        let Some(item) = self.items.get_mut(&id) else {
            return false;
        };
        item.history_entry.created_at = created_at;
        true
    }

    fn prune_peer_limit(&mut self, peer_device_id: &str) -> Vec<Uuid> {
        let mut peer_items = self
            .items
            .values()
            .filter(|item| item.source_device_id.as_deref() == Some(peer_device_id))
            .map(|item| (item.history_entry.id, item.history_entry.created_at))
            .collect::<Vec<_>>();
        peer_items.sort_by(|left, right| right.1.cmp(&left.1));

        let stale_ids = peer_items
            .into_iter()
            .skip(MAX_REMOTE_SESSION_ITEMS_PER_PEER)
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        self.remove_items(stale_ids)
    }

    fn prune_total_limit(&mut self) -> Vec<Uuid> {
        let mut items = self
            .items
            .values()
            .map(|item| (item.history_entry.id, item.history_entry.created_at))
            .collect::<Vec<_>>();
        items.sort_by(|left, right| right.1.cmp(&left.1));

        let stale_ids = items
            .into_iter()
            .skip(MAX_REMOTE_SESSION_ITEMS_TOTAL)
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        self.remove_items(stale_ids)
    }

    fn remove_items(&mut self, ids: Vec<Uuid>) -> Vec<Uuid> {
        if ids.is_empty() {
            return Vec::new();
        }

        let mut removed_ids = Vec::with_capacity(ids.len());
        let mut seen = HashSet::with_capacity(ids.len());
        for id in ids {
            if !seen.insert(id) {
                continue;
            }
            if self.items.remove(&id).is_some() {
                removed_ids.push(id);
            }
            self.content_cache.remove(id);
        }
        removed_ids
    }
}

impl RemoteHistoryItem {
    fn from_clipboard_item(item: &ClipboardItem) -> Self {
        let history_entry = AppController::build_remote_history_entry(item);
        let total_size_bytes = match &item.payload {
            ClipboardPayload::Text(_) => 0,
            ClipboardPayload::Files(files) => files.iter().map(|file| file.size_bytes).sum(),
        };

        Self {
            source_device_id: item.source_device_id.clone(),
            signature: item.signature.clone(),
            total_size_bytes,
            history_entry,
        }
    }

    pub(super) fn to_placeholder_item(&self) -> ClipboardItem {
        let payload = match self.history_entry.kind {
            ClipboardKind::Text => {
                ClipboardPayload::Text(self.history_entry.detail_tooltip.clone())
            }
            ClipboardKind::Files => ClipboardPayload::Files(Vec::new()),
        };

        ClipboardItem {
            id: self.history_entry.id,
            kind: self.history_entry.kind.clone(),
            summary: self.history_entry.summary_text.clone(),
            signature: self.signature.clone(),
            payload,
            source_device_id: self.source_device_id.clone(),
            source_device_name: Some(self.history_entry.source_tooltip.clone()),
            created_at: self.history_entry.created_at,
            is_remote: true,
            is_pinned: false,
        }
    }
}

impl RemoteContentCache {
    fn clear(&mut self) {
        self.items.clear();
        self.order.clear();
        self.total_bytes = 0;
    }

    fn get(&self, id: Uuid) -> Option<ClipboardItem> {
        self.items.get(&id).map(|entry| entry.item.clone())
    }

    fn insert(&mut self, item: ClipboardItem) {
        let estimated_bytes = estimate_clipboard_item_bytes(&item);
        if estimated_bytes > MAX_REMOTE_CONTENT_CACHE_BYTES {
            self.remove(item.id);
            return;
        }

        if let Some(existing) = self.items.remove(&item.id) {
            self.total_bytes = self.total_bytes.saturating_sub(existing.estimated_bytes);
            self.order.retain(|cached_id| *cached_id != item.id);
        }

        self.total_bytes = self.total_bytes.saturating_add(estimated_bytes);
        self.order.push_back(item.id);
        self.items.insert(
            item.id,
            CachedRemoteContent {
                item,
                estimated_bytes,
            },
        );

        while self.items.len() > MAX_REMOTE_CONTENT_CACHE_ITEMS
            || self.total_bytes > MAX_REMOTE_CONTENT_CACHE_BYTES
        {
            let Some(oldest_id) = self.order.pop_front() else {
                break;
            };
            if let Some(entry) = self.items.remove(&oldest_id) {
                self.total_bytes = self.total_bytes.saturating_sub(entry.estimated_bytes);
            }
        }
    }

    fn remove(&mut self, id: Uuid) {
        if let Some(entry) = self.items.remove(&id) {
            self.total_bytes = self.total_bytes.saturating_sub(entry.estimated_bytes);
        }
        self.order.retain(|cached_id| *cached_id != id);
    }
}

fn estimate_clipboard_item_bytes(item: &ClipboardItem) -> usize {
    let mut total = 256usize
        + item.summary.len()
        + item.signature.len()
        + item.source_device_id.as_deref().map_or(0, str::len)
        + item.source_device_name.as_deref().map_or(0, str::len);

    match &item.payload {
        ClipboardPayload::Text(text) => {
            total += text.len();
        }
        ClipboardPayload::Files(files) => {
            for file in files {
                total += file.name.len() + file.relative_path.len() + 64;
                total += file
                    .source_path
                    .as_ref()
                    .map(|path| path.as_os_str().to_string_lossy().len())
                    .unwrap_or(0);
                total += file
                    .local_path
                    .as_ref()
                    .map(|path| path.as_os_str().to_string_lossy().len())
                    .unwrap_or(0);
            }
        }
    }

    total
}

impl AppController {
    pub(super) fn build_remote_history_entry(item: &ClipboardItem) -> HistoryEntry {
        let search_blob = item
            .compact_search_blob(MAX_REMOTE_HISTORY_TOOLTIP_CHARS)
            .to_lowercase();
        HistoryEntry {
            id: item.id,
            kind: item.kind.clone(),
            created_at: item.created_at,
            source_device_id: item.source_device_id.clone(),
            source_badge: Self::item_source_badge_text(item),
            source_tooltip: Self::item_source_tooltip(item),
            summary_text: Self::item_summary_text(item),
            detail_tooltip: Self::item_detail_tooltip_with_limits(
                item,
                MAX_REMOTE_HISTORY_TOOLTIP_LINES,
                MAX_REMOTE_HISTORY_TOOLTIP_CHARS,
            ),
            search_blob,
            is_remote: true,
            is_pinned: false,
        }
    }
}
