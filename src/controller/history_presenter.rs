use std::collections::BTreeMap;

use super::*;
use crate::{
    constants::limits::{MAX_HISTORY_TOOLTIP_CHARS, MAX_HISTORY_TOOLTIP_LINES},
    core::model::{ClipboardItem, ClipboardKind, ClipboardPayload, FileDescriptor},
};

impl AppController {
    pub(super) fn history_entry_to_row(entry: &HistoryEntry) -> HistoryRow {
        HistoryRow {
            id: entry.id,
            kind: match entry.kind {
                ClipboardKind::Text => "text".to_string(),
                ClipboardKind::Files => "files".to_string(),
            },
            source_badge: entry.source_badge.clone(),
            source_tooltip: entry.source_tooltip.clone(),
            summary_text: entry.summary_text.clone(),
            detail_tooltip: entry.detail_tooltip.clone(),
            is_remote: entry.is_remote,
            is_pinned: entry.is_pinned,
        }
    }

    pub(super) fn build_history_entry(item: &ClipboardItem) -> HistoryEntry {
        let search_blob = item.compact_search_blob(512).to_lowercase();
        HistoryEntry {
            id: item.id,
            kind: item.kind.clone(),
            created_at: item.created_at,
            source_device_id: item.source_device_id.clone(),
            source_badge: Self::item_source_badge_text(item),
            source_tooltip: Self::item_source_tooltip(item),
            summary_text: Self::item_summary_text(item),
            detail_tooltip: Self::item_detail_tooltip(item),
            search_blob,
            is_remote: item.is_remote,
            is_pinned: item.is_pinned,
        }
    }

    pub(super) fn item_detail_tooltip(item: &ClipboardItem) -> String {
        Self::item_detail_tooltip_with_limits(
            item,
            MAX_HISTORY_TOOLTIP_LINES,
            MAX_HISTORY_TOOLTIP_CHARS,
        )
    }

    pub(super) fn item_detail_tooltip_with_limits(
        item: &ClipboardItem,
        max_lines: usize,
        max_chars: usize,
    ) -> String {
        match &item.payload {
            ClipboardPayload::Text(text) => {
                if text.trim().is_empty() {
                    "空文本内容".to_string()
                } else {
                    Self::limit_tooltip_text_with_limits(text.as_str(), max_lines, max_chars)
                }
            }
            ClipboardPayload::Files(files) => Self::limit_tooltip_text_with_limits(
                &Self::files_content_tooltip(files.as_slice()),
                max_lines,
                max_chars,
            ),
        }
    }

    pub(super) fn item_source_badge_text(item: &ClipboardItem) -> String {
        if item.is_remote {
            item.source_device_name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(Self::truncate_badge_label)
                .unwrap_or_else(|| "远端".to_string())
        } else {
            "本机".to_string()
        }
    }

    pub(super) fn item_source_tooltip(item: &ClipboardItem) -> String {
        if item.is_remote {
            item.source_device_name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or("远端设备")
                .to_string()
        } else {
            item.source_device_name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or("本机")
                .to_string()
        }
    }

    pub(super) fn item_summary_text(item: &ClipboardItem) -> String {
        match &item.payload {
            ClipboardPayload::Text(text) => text
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(|line| Self::truncate_inline(line, 240))
                .unwrap_or_else(|| "空文本内容".to_string()),
            ClipboardPayload::Files(files) => {
                if files.is_empty() {
                    return "无文件内容".to_string();
                }

                if files.len() == 1 {
                    return Self::truncate_inline(
                        &format!("文件 · {}", files[0].relative_path),
                        240,
                    );
                }

                let lead = files
                    .first()
                    .map(|file| file.relative_path.as_str())
                    .filter(|path| !path.trim().is_empty())
                    .unwrap_or("文件");
                Self::truncate_inline(&format!("{} 个文件 · {}", files.len(), lead), 240)
            }
        }
    }

    pub(super) fn files_content_tooltip(files: &[FileDescriptor]) -> String {
        if files.is_empty() {
            return "无文件内容".to_string();
        }

        #[derive(Default)]
        struct TopLevelEntry {
            name: String,
            display_path: String,
            is_directory: bool,
            file_count: usize,
            total_bytes: u64,
        }

        let mut entries = BTreeMap::<String, TopLevelEntry>::new();
        for file in files {
            let relative_path = file.relative_path.trim().trim_matches('/');
            if relative_path.is_empty() {
                continue;
            }

            let mut parts = relative_path.split('/');
            let top_level = parts.next().unwrap_or(relative_path);
            let has_nested = parts.next().is_some();
            let entry = entries
                .entry(top_level.to_string())
                .or_insert_with(|| TopLevelEntry {
                    name: top_level.to_string(),
                    display_path: relative_path.to_string(),
                    is_directory: has_nested,
                    file_count: 0,
                    total_bytes: 0,
                });
            entry.is_directory |= has_nested;
            entry.file_count += 1;
            entry.total_bytes = entry.total_bytes.saturating_add(file.size_bytes);
            if entry.display_path.len() > relative_path.len() {
                entry.display_path = relative_path.to_string();
            }
        }

        if entries.is_empty() {
            return "无文件内容".to_string();
        }

        if entries.len() == 1 {
            let entry = entries.into_values().next().unwrap_or_default();
            if entry.is_directory {
                return format!(
                    "文件夹\n名称: {}\n包含文件: {}\n总大小: {}",
                    entry.name,
                    entry.file_count,
                    Self::format_bytes(entry.total_bytes)
                );
            }

            return format!(
                "文件\n名称: {}\n路径: {}\n大小: {}",
                entry.name,
                entry.display_path,
                Self::format_bytes(entry.total_bytes)
            );
        }

        let mut lines = Vec::with_capacity(entries.len() + 1);
        lines.push(format!("共 {} 项", entries.len()));
        for entry in entries.into_values() {
            if entry.is_directory {
                lines.push(format!(
                    "文件夹 · {} · {} 个文件 · {}",
                    entry.name,
                    entry.file_count,
                    Self::format_bytes(entry.total_bytes)
                ));
            } else {
                lines.push(format!(
                    "文件 · {} · {}",
                    entry.display_path,
                    Self::format_bytes(entry.total_bytes)
                ));
            }
        }
        lines.join("\n")
    }

    pub(super) fn truncate_badge_label(text: &str) -> String {
        Self::truncate_inline(text, 8)
    }

    pub(super) fn truncate_inline(text: &str, max_chars: usize) -> String {
        let trimmed = text.trim();
        if trimmed.is_empty() || max_chars == 0 {
            return String::new();
        }

        let mut value = trimmed.chars().take(max_chars).collect::<String>();
        if trimmed.chars().count() > max_chars {
            value.push('…');
        }
        value
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn limit_tooltip_text(text: &str) -> String {
        Self::limit_tooltip_text_with_limits(
            text,
            MAX_HISTORY_TOOLTIP_LINES,
            MAX_HISTORY_TOOLTIP_CHARS,
        )
    }

    pub(super) fn limit_tooltip_text_with_limits(
        text: &str,
        max_lines: usize,
        max_chars: usize,
    ) -> String {
        if text.is_empty() {
            return String::new();
        }

        let mut clipped = String::with_capacity(text.len().min(max_chars));
        let mut clipped_chars = 0usize;
        let mut seen_lines = 0usize;
        let mut omitted_lines = 0usize;
        let mut truncated_by_chars = false;

        for line in text.lines() {
            if seen_lines >= max_lines {
                omitted_lines += 1;
                continue;
            }

            if !clipped.is_empty() {
                if clipped_chars >= max_chars {
                    truncated_by_chars = true;
                    omitted_lines += 1;
                    continue;
                }
                clipped.push('\n');
                clipped_chars += 1;
            }

            let remaining_chars = max_chars.saturating_sub(clipped_chars);
            if remaining_chars == 0 {
                truncated_by_chars = true;
                omitted_lines += 1;
                continue;
            }

            let line_len = line.chars().count();
            if line_len <= remaining_chars {
                clipped.push_str(line);
                clipped_chars += line_len;
            } else {
                for (index, ch) in line.chars().enumerate() {
                    if index + 1 >= remaining_chars {
                        break;
                    }
                    clipped.push(ch);
                    clipped_chars += 1;
                }
                clipped.push('…');
                clipped_chars += 1;
                truncated_by_chars = true;
            }

            seen_lines += 1;
        }

        if omitted_lines > 0 {
            if !clipped.is_empty() {
                clipped.push('\n');
            }
            clipped.push_str(&format!("……已省略 {omitted_lines} 行"));
        } else if truncated_by_chars {
            if !clipped.is_empty() {
                clipped.push('\n');
            }
            clipped.push_str("……内容已截断");
        }

        clipped
    }

    pub(super) fn format_bytes(bytes: u64) -> String {
        const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

        let mut value = bytes as f64;
        let mut unit_index = 0usize;
        while value >= 1024.0 && unit_index + 1 < UNITS.len() {
            value /= 1024.0;
            unit_index += 1;
        }

        if unit_index == 0 {
            format!("{bytes} {}", UNITS[unit_index])
        } else if value >= 100.0 {
            format!("{value:.0} {}", UNITS[unit_index])
        } else if value >= 10.0 {
            format!("{value:.1} {}", UNITS[unit_index])
        } else {
            format!("{value:.2} {}", UNITS[unit_index])
        }
    }
}
