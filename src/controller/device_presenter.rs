use std::collections::HashSet;

use super::*;
use crate::core::model::DiscoveredPeer;
use time::{OffsetDateTime, macros::format_description};

impl AppController {
    pub(crate) fn device_detail_text(address: Option<(&str, u16)>) -> String {
        match address {
            Some((host, port)) => format!("{host}:{port}"),
            None => String::new(),
        }
    }

    pub(crate) fn device_status_label(status: DeviceStatusKind) -> &'static str {
        match status {
            DeviceStatusKind::ConnectedTrusted => "已信任已连接",
            DeviceStatusKind::TrustedStandby => "已信任未连接",
            DeviceStatusKind::Offline => "离线",
            DeviceStatusKind::Untrusted => "未信任",
        }
    }

    fn device_status_kind(
        &self,
        device_id: &str,
        is_online: bool,
        is_trusted: bool,
    ) -> DeviceStatusKind {
        if !is_trusted {
            return DeviceStatusKind::Untrusted;
        }
        if !is_online {
            return DeviceStatusKind::Offline;
        }
        if self.remote_share_enabled.get(device_id).copied() == Some(false) {
            DeviceStatusKind::TrustedStandby
        } else {
            DeviceStatusKind::ConnectedTrusted
        }
    }

    pub(crate) fn device_status_tooltip(
        status: DeviceStatusKind,
        is_online: bool,
        last_seen: Option<OffsetDateTime>,
    ) -> String {
        let status_text = Self::device_status_label(status);
        let seen_label = if is_online {
            "最近发现"
        } else {
            "最后发现"
        };

        match last_seen {
            Some(last_seen) => format!(
                "状态：{status_text}\n{seen_label}：{}\n记录时间：{}",
                Self::format_relative_time(last_seen),
                Self::format_seen_time(last_seen)
            ),
            None => format!("状态：{status_text}\n{seen_label}：暂无记录"),
        }
    }

    fn format_relative_time(last_seen: OffsetDateTime) -> String {
        let elapsed =
            (OffsetDateTime::now_utc().unix_timestamp() - last_seen.unix_timestamp()).max(0);

        match elapsed {
            0..=59 => "刚刚".to_string(),
            60..=3599 => format!("{} 分钟前", elapsed / 60),
            3600..=86_399 => format!("{} 小时前", elapsed / 3_600),
            86_400..=2_592_000 => format!("{} 天前", elapsed / 86_400),
            _ => Self::format_seen_time(last_seen),
        }
    }

    fn format_seen_time(last_seen: OffsetDateTime) -> String {
        last_seen
            .format(format_description!(
                "[year]-[month]-[day] [hour]:[minute]:[second] UTC"
            ))
            .unwrap_or_else(|_| "未知时间".to_string())
    }

    fn device_status_sort_rank(status: DeviceStatusKind) -> u8 {
        match status {
            DeviceStatusKind::ConnectedTrusted => 0,
            DeviceStatusKind::TrustedStandby => 1,
            DeviceStatusKind::Offline => 2,
            DeviceStatusKind::Untrusted => 3,
        }
    }

    pub(super) fn discovered_peer_is_trusted(
        peer: &DiscoveredPeer,
        trusted_fingerprints: &HashSet<&str>,
    ) -> bool {
        !peer.fingerprint.is_empty() && trusted_fingerprints.contains(peer.fingerprint.as_str())
    }

    fn collapse_shadowed_offline_device_entries(
        entries: Vec<SettingsDeviceEntry>,
    ) -> Vec<SettingsDeviceEntry> {
        let online_trusted_names = entries
            .iter()
            .filter(|entry| entry.is_online && entry.is_trusted)
            .map(|entry| entry.device_name.trim().to_string())
            .filter(|device_name| !device_name.is_empty())
            .collect::<HashSet<_>>();
        if online_trusted_names.is_empty() {
            return entries;
        }

        entries
            .into_iter()
            .filter(|entry| {
                let device_name = entry.device_name.trim();
                let is_shadowed_offline_alias = entry.is_trusted
                    && !entry.is_online
                    && entry.status_kind == DeviceStatusKind::Offline
                    && entry.secondary_text.trim().is_empty()
                    && entry.status_tooltip.contains("暂无记录")
                    && !device_name.is_empty()
                    && online_trusted_names.contains(device_name);
                !is_shadowed_offline_alias
            })
            .collect()
    }

    pub(super) fn collapse_duplicate_device_entries(
        entries: Vec<SettingsDeviceEntry>,
    ) -> Vec<SettingsDeviceEntry> {
        let mut collapsed = std::collections::BTreeMap::<String, SettingsDeviceEntry>::new();

        for entry in entries {
            match collapsed.get(&entry.device_id) {
                Some(existing)
                    if Self::device_status_sort_rank(existing.status_kind)
                        <= Self::device_status_sort_rank(entry.status_kind)
                        && (existing.is_online || !entry.is_online)
                        && (existing.is_trusted || !entry.is_trusted)
                        && (!existing.secondary_text.is_empty()
                            || entry.secondary_text.is_empty()) => {}
                _ => {
                    collapsed.insert(entry.device_id.clone(), entry);
                }
            }
        }

        let mut entries =
            Self::collapse_shadowed_offline_device_entries(collapsed.into_values().collect());
        entries.sort_by(|left, right| {
            Self::device_status_sort_rank(left.status_kind)
                .cmp(&Self::device_status_sort_rank(right.status_kind))
                .then_with(|| left.device_name.cmp(&right.device_name))
                .then_with(|| left.device_id.cmp(&right.device_id))
        });
        entries
    }

    pub(super) fn device_entries(&self) -> Vec<SettingsDeviceEntry> {
        let trusted_fingerprints = self
            .config
            .trusted_peers
            .iter()
            .map(|peer| peer.fingerprint.as_str())
            .filter(|fingerprint| !fingerprint.is_empty())
            .collect::<HashSet<_>>();
        let mut seen = HashSet::new();
        let mut seen_fingerprints = HashSet::new();
        let mut entries = self
            .discovered
            .iter()
            .map(|peer| {
                seen.insert(peer.device_id.clone());
                if !peer.fingerprint.is_empty() {
                    seen_fingerprints.insert(peer.fingerprint.clone());
                }
                let is_trusted = Self::discovered_peer_is_trusted(peer, &trusted_fingerprints);
                let last_seen = self
                    .device_last_seen
                    .get(&peer.device_id)
                    .copied()
                    .or(Some(peer.last_seen));
                let status_kind = self.device_status_kind(&peer.device_id, true, is_trusted);
                SettingsDeviceEntry {
                    device_id: peer.device_id.clone(),
                    device_name: peer.device_name.clone(),
                    secondary_text: Self::device_detail_text(Some((&peer.address, peer.port))),
                    status_kind,
                    is_online: true,
                    is_trusted,
                    status_tooltip: Self::device_status_tooltip(status_kind, true, last_seen),
                }
            })
            .collect::<Vec<_>>();

        for peer in &self.config.trusted_peers {
            if seen.contains(&peer.device_id)
                || (!peer.fingerprint.is_empty() && seen_fingerprints.contains(&peer.fingerprint))
            {
                continue;
            }
            let status_kind = self.device_status_kind(&peer.device_id, false, true);
            entries.push(SettingsDeviceEntry {
                device_id: peer.device_id.clone(),
                device_name: peer.device_name.clone(),
                secondary_text: Self::device_detail_text(None),
                status_kind,
                is_online: false,
                is_trusted: true,
                status_tooltip: Self::device_status_tooltip(
                    status_kind,
                    false,
                    self.device_last_seen.get(&peer.device_id).copied(),
                ),
            });
            seen.insert(peer.device_id.clone());
            if !peer.fingerprint.is_empty() {
                seen_fingerprints.insert(peer.fingerprint.clone());
            }
        }

        entries.sort_by(|left, right| {
            Self::device_status_sort_rank(left.status_kind)
                .cmp(&Self::device_status_sort_rank(right.status_kind))
                .then_with(|| left.device_name.cmp(&right.device_name))
        });
        Self::collapse_duplicate_device_entries(entries)
    }
}
