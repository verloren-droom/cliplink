use super::*;
use crate::{
    constants::limits::{MAX_REMOTE_SESSION_ITEMS_PER_PEER, MAX_REMOTE_SESSION_ITEMS_TOTAL},
    core::{
        at_rest::LocalDataCipher,
        clipboard::ClipboardBackend,
        error::AppResult,
        model::{ClipboardItem, ClipboardKind, ClipboardPayload, DiscoveredPeer, TrustedPeer},
        paths::AppPaths,
        storage::{HistoryStore, HistoryStoreProfile},
    },
};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::Instant,
};
use time::OffsetDateTime;
use uuid::Uuid;

struct TestClipboard;

impl ClipboardBackend for TestClipboard {
    fn poll(&self, _: &str, _: &str) -> AppResult<Option<ClipboardItem>> {
        Ok(None)
    }

    fn write_item(&self, _: &ClipboardItem) -> AppResult<()> {
        Ok(())
    }
}

fn unique_test_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cliplink-{name}-{}", Uuid::new_v4()))
}

fn build_test_controller(name: &str) -> AppController {
    let root = unique_test_root(name);
    let paths = AppPaths::from_root(root);
    paths.ensure().unwrap();

    let cipher = LocalDataCipher::from_bytes([7_u8; 32]);
    let config_store = crate::core::config::ConfigStore::new(&paths, cipher.clone());
    let history_store =
        HistoryStore::open(&paths, cipher.clone(), HistoryStoreProfile::default()).unwrap();

    AppController {
        services: ControllerServices {
            paths,
            local_data_cipher: cipher,
            config_store,
            history_store,
            history_store_profile: HistoryStoreProfile::default(),
            clipboard: Box::new(TestClipboard),
            runtime: None,
            runtime_policy: ControllerRuntimePolicy::default(),
        },
        config: crate::core::config::AppConfig::new(Some("Local Mac")),
        local_history: Vec::new(),
        remote_history: super::remote::RemoteHistorySession::default(),
        discovered: Vec::new(),
        device_last_seen: HashMap::new(),
        remote_share_enabled: HashMap::new(),
        status: None,
        last_clipboard_poll: Instant::now(),
        preferences_visible: false,
        pending_trust_requests: Vec::new(),
        requested_history_sync_peers: HashSet::new(),
        announced_share_state_peers: HashSet::new(),
        pending_remote_activation: None,
        active_transfer: None,
    }
}

#[test]
fn truncate_inline_appends_ellipsis_when_needed() {
    assert_eq!(AppController::truncate_inline("abcdef", 3), "abc…");
    assert_eq!(AppController::truncate_inline("ab", 3), "ab");
}

#[test]
fn limit_tooltip_text_adds_omission_notice_for_extra_lines() {
    let text = (0..40)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let clipped = AppController::limit_tooltip_text(&text);

    assert!(clipped.contains("line 0"));
    assert!(clipped.contains("……已省略"));
}

#[test]
fn format_bytes_uses_human_readable_units() {
    assert_eq!(AppController::format_bytes(999), "999 B");
    assert_eq!(AppController::format_bytes(1024), "1.00 KB");
    assert_eq!(AppController::format_bytes(5 * 1024 * 1024), "5.00 MB");
}

#[test]
fn device_status_tooltip_shows_offline_without_last_seen_record() {
    let tooltip = AppController::device_status_tooltip(DeviceStatusKind::Offline, false, None);

    assert!(tooltip.contains("状态：离线"));
    assert!(tooltip.contains("最后发现：暂无记录"));
}

#[test]
fn device_status_label_maps_status_kinds() {
    assert_eq!(
        AppController::device_status_label(DeviceStatusKind::ConnectedTrusted),
        "已信任已连接"
    );
    assert_eq!(
        AppController::device_status_label(DeviceStatusKind::TrustedStandby),
        "已信任未连接"
    );
    assert_eq!(
        AppController::device_status_label(DeviceStatusKind::Offline),
        "离线"
    );
    assert_eq!(
        AppController::device_status_label(DeviceStatusKind::Untrusted),
        "未信任"
    );
}

#[test]
fn device_detail_text_keeps_only_address_for_row_subtitle() {
    let detail = AppController::device_detail_text(Some(("192.168.1.8", 27841)));

    assert_eq!(detail, "192.168.1.8:27841");
}

#[test]
fn duplicate_device_entries_collapse_by_exact_device_id() {
    let entries = vec![
        SettingsDeviceEntry {
            device_id: "same-id".to_string(),
            device_name: "HUAWEI NOH-AN01".to_string(),
            secondary_text: "192.168.1.4:27841".to_string(),
            status_kind: DeviceStatusKind::ConnectedTrusted,
            is_online: true,
            is_trusted: true,
            status_tooltip: String::new(),
        },
        SettingsDeviceEntry {
            device_id: "same-id".to_string(),
            device_name: "HUAWEI NOH-AN01".to_string(),
            secondary_text: String::new(),
            status_kind: DeviceStatusKind::Offline,
            is_online: false,
            is_trusted: true,
            status_tooltip: String::new(),
        },
    ];

    let collapsed = AppController::collapse_duplicate_device_entries(entries);

    assert_eq!(collapsed.len(), 1);
    assert_eq!(collapsed[0].device_id, "same-id");
    assert_eq!(collapsed[0].status_kind, DeviceStatusKind::ConnectedTrusted);
}

#[test]
fn device_entries_with_same_name_but_different_device_ids_are_preserved() {
    let entries = vec![
        SettingsDeviceEntry {
            device_id: "device-a".to_string(),
            device_name: "HUAWEI NOH-AN01".to_string(),
            secondary_text: "192.168.1.4:27841".to_string(),
            status_kind: DeviceStatusKind::ConnectedTrusted,
            is_online: true,
            is_trusted: true,
            status_tooltip: String::new(),
        },
        SettingsDeviceEntry {
            device_id: "device-b".to_string(),
            device_name: "HUAWEI NOH-AN01".to_string(),
            secondary_text: String::new(),
            status_kind: DeviceStatusKind::Offline,
            is_online: false,
            is_trusted: true,
            status_tooltip: String::new(),
        },
    ];

    let collapsed = AppController::collapse_duplicate_device_entries(entries);

    assert_eq!(collapsed.len(), 2);
}

#[test]
fn discovered_peer_trust_requires_matching_fingerprint() {
    let trusted_fingerprints = ["fp-1"].into_iter().collect::<HashSet<_>>();
    let trusted_peer = DiscoveredPeer {
        device_id: "same-id".to_string(),
        device_name: "HUAWEI NOH-AN01".to_string(),
        host: "peer.local".to_string(),
        address: "192.168.1.4".to_string(),
        port: 27_841,
        fingerprint: "fp-1".to_string(),
        last_seen: OffsetDateTime::UNIX_EPOCH,
    };
    let mismatched_peer = DiscoveredPeer {
        fingerprint: "fp-2".to_string(),
        ..trusted_peer.clone()
    };

    assert!(AppController::discovered_peer_is_trusted(
        &trusted_peer,
        &trusted_fingerprints,
    ));
    assert!(!AppController::discovered_peer_is_trusted(
        &mismatched_peer,
        &trusted_fingerprints,
    ));
}

#[test]
fn shadowed_offline_same_name_without_last_seen_is_collapsed() {
    let entries = vec![
        SettingsDeviceEntry {
            device_id: "device-a".to_string(),
            device_name: "HUAWEI NOH-AN01".to_string(),
            secondary_text: "192.168.1.4:27841".to_string(),
            status_kind: DeviceStatusKind::ConnectedTrusted,
            is_online: true,
            is_trusted: true,
            status_tooltip: "状态：已信任已连接\n最近发现：刚刚".to_string(),
        },
        SettingsDeviceEntry {
            device_id: "device-b".to_string(),
            device_name: "HUAWEI NOH-AN01".to_string(),
            secondary_text: String::new(),
            status_kind: DeviceStatusKind::Offline,
            is_online: false,
            is_trusted: true,
            status_tooltip: "状态：离线\n最后发现：暂无记录".to_string(),
        },
    ];

    let collapsed = AppController::collapse_duplicate_device_entries(entries);

    assert_eq!(collapsed.len(), 1);
    assert_eq!(collapsed[0].device_id, "device-a");
}

#[test]
fn previously_seen_offline_same_name_device_is_preserved() {
    let entries = vec![
        SettingsDeviceEntry {
            device_id: "device-a".to_string(),
            device_name: "HUAWEI NOH-AN01".to_string(),
            secondary_text: "192.168.1.4:27841".to_string(),
            status_kind: DeviceStatusKind::ConnectedTrusted,
            is_online: true,
            is_trusted: true,
            status_tooltip: "状态：已信任已连接\n最近发现：刚刚".to_string(),
        },
        SettingsDeviceEntry {
            device_id: "device-b".to_string(),
            device_name: "HUAWEI NOH-AN01".to_string(),
            secondary_text: String::new(),
            status_kind: DeviceStatusKind::Offline,
            is_online: false,
            is_trusted: true,
            status_tooltip: "状态：离线\n最后发现：2026-03-31 22:00:00 UTC".to_string(),
        },
    ];

    let collapsed = AppController::collapse_duplicate_device_entries(entries);

    assert_eq!(collapsed.len(), 2);
}

#[test]
fn offline_trusted_duplicates_with_same_fingerprint_are_not_listed_multiple_times() {
    let mut controller = build_test_controller("offline-trusted-dedup");
    controller.config.trusted_peers = vec![
        TrustedPeer {
            device_id: "device-old".to_string(),
            device_name: "HUAWEI NOH-AN01".to_string(),
            fingerprint: "fp-1".to_string(),
        },
        TrustedPeer {
            device_id: "device-new".to_string(),
            device_name: "HUAWEI NOH-AN01".to_string(),
            fingerprint: "fp-1".to_string(),
        },
    ];

    let entries = controller.device_entries();
    let root = controller.services.paths.root.clone();
    drop(controller);
    let _ = std::fs::remove_dir_all(root);

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].device_name, "HUAWEI NOH-AN01");
    assert_eq!(entries[0].status_kind, DeviceStatusKind::Offline);
}

fn sample_remote_text_item(device_id: &str, index: i64) -> ClipboardItem {
    ClipboardItem {
        id: Uuid::new_v4(),
        kind: ClipboardKind::Text,
        summary: format!("remote {index}"),
        signature: format!("sig-{device_id}-{index}"),
        payload: ClipboardPayload::Text(format!("payload {index}")),
        source_device_id: Some(device_id.to_string()),
        source_device_name: Some(format!("Device {device_id}")),
        created_at: OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(index),
        is_remote: true,
        is_pinned: false,
    }
}

#[test]
fn remote_history_session_limits_items_per_peer() {
    let mut session = super::remote::RemoteHistorySession::default();
    let items = (0..(MAX_REMOTE_SESSION_ITEMS_PER_PEER as i64 + 3))
        .map(|index| sample_remote_text_item("peer-a", index))
        .collect::<Vec<_>>();

    let removed_ids = session.replace_peer_items("peer-a", items);

    assert_eq!(session.len(), MAX_REMOTE_SESSION_ITEMS_PER_PEER);
    assert!(removed_ids.is_empty());
}

#[test]
fn remote_history_session_reports_replaced_peer_items_for_cleanup() {
    let mut session = super::remote::RemoteHistorySession::default();
    let first = sample_remote_text_item("peer-a", 1);
    let second = sample_remote_text_item("peer-a", 2);
    session.replace_peer_items("peer-a", vec![first.clone(), second.clone()]);

    let replacement = sample_remote_text_item("peer-a", 3);
    let mut removed_ids = session.replace_peer_items("peer-a", vec![replacement.clone()]);
    removed_ids.sort_unstable();
    let mut expected_ids = vec![first.id, second.id];
    expected_ids.sort_unstable();

    assert_eq!(removed_ids, expected_ids);
    assert!(session.item(replacement.id).is_some());
}

#[test]
fn remote_history_session_prunes_offline_devices_and_total_budget() {
    let mut session = super::remote::RemoteHistorySession::default();
    for index in 0..(MAX_REMOTE_SESSION_ITEMS_TOTAL as i64 + 10) {
        let peer = if index % 2 == 0 { "peer-a" } else { "peer-b" };
        let _ = session.upsert_live_item(sample_remote_text_item(peer, index));
    }

    assert!(session.len() <= MAX_REMOTE_SESSION_ITEMS_TOTAL);

    let online_ids = ["peer-a".to_string()].into_iter().collect();
    assert!(!session.retain_online_devices(&online_ids).is_empty());
    assert!(
        session
            .entries()
            .all(|entry| !entry.source_tooltip.contains("peer-b"))
    );
}

#[test]
fn remote_history_session_only_removes_items_for_matching_peer() {
    let mut session = super::remote::RemoteHistorySession::default();
    let peer_a = sample_remote_text_item("peer-a", 1);
    let peer_b = sample_remote_text_item("peer-b", 2);
    let _ = session.upsert_live_item(peer_a.clone());
    let _ = session.upsert_live_item(peer_b.clone());

    let removed = session.remove_peer_items("peer-a", &[peer_a.id, peer_b.id]);

    assert_eq!(removed, vec![peer_a.id]);
    assert!(session.item(peer_a.id).is_none());
    assert!(session.item(peer_b.id).is_some());
}

#[test]
fn history_scope_parsing_accepts_known_values_and_defaults_to_all() {
    assert_eq!(HistoryScope::from_raw("all"), HistoryScope::All);
    assert_eq!(HistoryScope::from_raw("local"), HistoryScope::Local);
    assert_eq!(
        HistoryScope::from_raw("device:peer-a"),
        HistoryScope::Device("peer-a".to_string())
    );
    assert_eq!(HistoryScope::from_raw("unknown"), HistoryScope::All);
    assert_eq!(
        HistoryScope::from_raw("  device:peer-b  "),
        HistoryScope::Device("peer-b".to_string())
    );
}
