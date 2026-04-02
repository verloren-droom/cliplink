use std::{collections::BTreeSet, path::PathBuf};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClipboardKind {
    Text,
    Files,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileDescriptor {
    pub name: String,
    pub relative_path: String,
    pub size_bytes: u64,
    pub source_path: Option<PathBuf>,
    pub local_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClipboardPayload {
    Text(String),
    Files(Vec<FileDescriptor>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClipboardItem {
    pub id: Uuid,
    pub kind: ClipboardKind,
    pub summary: String,
    pub signature: String,
    pub payload: ClipboardPayload,
    pub source_device_id: Option<String>,
    pub source_device_name: Option<String>,
    pub created_at: OffsetDateTime,
    pub is_remote: bool,
    #[serde(default)]
    pub is_pinned: bool,
}

impl ClipboardItem {
    pub fn compact_search_blob(&self, max_chars: usize) -> String {
        let limit = max_chars.max(32);
        let mut out = String::new();
        push_limited(&mut out, self.summary.trim(), limit);

        match &self.payload {
            ClipboardPayload::Text(text) => {
                if out.chars().count() < limit {
                    push_limited(&mut out, text.trim(), limit);
                }
            }
            ClipboardPayload::Files(files) => {
                for file in files {
                    if out.chars().count() >= limit {
                        break;
                    }
                    push_limited(&mut out, file.relative_path.trim(), limit);
                }
            }
        }

        out
    }

    pub fn as_clipboard_text(&self) -> String {
        match &self.payload {
            ClipboardPayload::Text(text) => text.clone(),
            ClipboardPayload::Files(files) => files
                .iter()
                .filter_map(|file| {
                    file.local_path
                        .as_ref()
                        .or(file.source_path.as_ref())
                        .map(|path| path.to_string_lossy().to_string())
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub fn local_file_parent_directories(&self) -> Vec<PathBuf> {
        let ClipboardPayload::Files(files) = &self.payload else {
            return Vec::new();
        };

        let mut directories = BTreeSet::new();
        for file in files {
            let Some(parent) = file
                .local_path
                .as_ref()
                .or(file.source_path.as_ref())
                .and_then(|path| path.parent())
                .filter(|path| !path.as_os_str().is_empty())
            else {
                continue;
            };
            directories.insert(parent.to_path_buf());
        }

        directories.into_iter().collect()
    }
}

fn push_limited(out: &mut String, value: &str, max_chars: usize) {
    if value.is_empty() || out.chars().count() >= max_chars {
        return;
    }

    if !out.is_empty() {
        out.push(' ');
    }

    let remaining = max_chars.saturating_sub(out.chars().count());
    let mut chars = value.chars();
    for _ in 0..remaining {
        let Some(ch) = chars.next() else {
            return;
        };
        out.push(ch);
    }

    if chars.next().is_some() {
        out.pop();
        out.push('…');
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustedPeer {
    pub device_id: String,
    pub device_name: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredPeer {
    pub device_id: String,
    pub device_name: String,
    pub host: String,
    pub address: String,
    pub port: u16,
    pub fingerprint: String,
    pub last_seen: OffsetDateTime,
}
