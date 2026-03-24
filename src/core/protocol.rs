use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::core::model::{ClipboardItem, ClipboardKind, ClipboardPayload, FileDescriptor};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferHeader {
    pub protocol_version: u8,
    pub item: WireClipboardItem,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireClipboardItem {
    pub id: Uuid,
    pub signature: String,
    pub kind: ClipboardKind,
    pub summary: String,
    #[serde(default)]
    pub preview: String,
    pub text: Option<String>,
    pub files: Vec<WireFile>,
    pub source_device_id: Option<String>,
    pub source_device_name: Option<String>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireFile {
    pub name: String,
    pub relative_path: String,
    pub size_bytes: u64,
}

impl TransferHeader {
    pub fn from_item(item: &ClipboardItem) -> Self {
        let (text, files) = match &item.payload {
            ClipboardPayload::Text(text) => (Some(text.clone()), Vec::new()),
            ClipboardPayload::Files(files) => (
                None,
                files
                    .iter()
                    .map(|file| WireFile {
                        name: file.name.clone(),
                        relative_path: file.relative_path.clone(),
                        size_bytes: file.size_bytes,
                    })
                    .collect(),
            ),
        };

        Self {
            protocol_version: 1,
            item: WireClipboardItem {
                id: item.id,
                signature: item.signature.clone(),
                kind: item.kind.clone(),
                summary: item.summary.clone(),
                preview: String::new(),
                text,
                files,
                source_device_id: item.source_device_id.clone(),
                source_device_name: item.source_device_name.clone(),
                created_at: item.created_at,
            },
        }
    }

    pub fn into_item(self, local_files: Vec<PathBuf>) -> ClipboardItem {
        let payload = match self.item.kind {
            ClipboardKind::Text => ClipboardPayload::Text(self.item.text.unwrap_or_default()),
            ClipboardKind::Files => ClipboardPayload::Files(
                self.item
                    .files
                    .into_iter()
                    .zip(local_files)
                    .map(|(file, local_path)| FileDescriptor {
                        name: file.name,
                        relative_path: file.relative_path,
                        size_bytes: file.size_bytes,
                        source_path: None,
                        local_path: Some(local_path),
                    })
                    .collect(),
            ),
        };

        ClipboardItem {
            id: self.item.id,
            kind: self.item.kind,
            summary: self.item.summary,
            signature: self.item.signature,
            payload,
            source_device_id: self.item.source_device_id,
            source_device_name: self.item.source_device_name,
            created_at: self.item.created_at,
            is_remote: true,
            is_pinned: false,
        }
    }
}
