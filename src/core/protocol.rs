use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::core::{
    error::{AppError, AppResult},
    model::{ClipboardItem, ClipboardKind, ClipboardPayload, FileDescriptor},
};

/// Version identifier for the current QUIC clipboard transport protocol.
pub const TRANSFER_PROTOCOL_VERSION: u8 = 4;

/// Length-prefixed request envelope sent from a client peer to a remote peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferRequest {
    pub protocol_version: u8,
    pub body: TransferRequestBody,
}

/// Request variants supported by the transport protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransferRequestBody {
    PushClipboard {
        item: WireClipboardItem,
        include_files: bool,
    },
    RemoveHistoryItems {
        item_ids: Vec<Uuid>,
    },
    UpdateShareState {
        share_local_history: bool,
    },
    RevokeTrust,
    FetchHistorySnapshot {
        limit: usize,
    },
    FetchClipboardItem {
        item_id: Uuid,
    },
    FetchFiles {
        item_id: Uuid,
    },
}

/// Length-prefixed response envelope sent back on request/response streams.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferResponse {
    pub protocol_version: u8,
    pub body: TransferResponseBody,
}

/// Response variants supported by the transport protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransferResponseBody {
    Ack,
    HistorySnapshot {
        items: Vec<WireClipboardItem>,
    },
    ClipboardItem {
        item: WireClipboardItem,
        include_files: bool,
    },
    Error {
        message: String,
    },
}

/// Wire-safe clipboard item payload without any host-local filesystem paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireClipboardItem {
    pub id: Uuid,
    pub signature: String,
    pub kind: ClipboardKind,
    pub summary: String,
    pub text: Option<String>,
    pub files: Vec<WireFile>,
    pub source_device_id: Option<String>,
    pub source_device_name: Option<String>,
    pub created_at: OffsetDateTime,
}

/// Wire-safe file descriptor that only carries metadata needed for transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireFile {
    pub name: String,
    pub relative_path: String,
    pub size_bytes: u64,
}

impl TransferRequest {
    pub fn push_clipboard(item: &ClipboardItem) -> Self {
        Self {
            protocol_version: TRANSFER_PROTOCOL_VERSION,
            body: TransferRequestBody::PushClipboard {
                item: WireClipboardItem::from_item(item),
                // Live clipboard broadcasts only advertise file metadata.
                // The actual file bytes stay on-demand and are fetched when the user activates
                // the remote history item.
                include_files: false,
            },
        }
    }

    pub fn fetch_history_snapshot(limit: usize) -> Self {
        Self {
            protocol_version: TRANSFER_PROTOCOL_VERSION,
            body: TransferRequestBody::FetchHistorySnapshot {
                limit: limit.max(1),
            },
        }
    }

    pub fn revoke_trust() -> Self {
        Self {
            protocol_version: TRANSFER_PROTOCOL_VERSION,
            body: TransferRequestBody::RevokeTrust,
        }
    }

    pub fn remove_history_items(item_ids: &[Uuid]) -> Self {
        Self {
            protocol_version: TRANSFER_PROTOCOL_VERSION,
            body: TransferRequestBody::RemoveHistoryItems {
                item_ids: item_ids.to_vec(),
            },
        }
    }

    pub fn update_share_state(share_local_history: bool) -> Self {
        Self {
            protocol_version: TRANSFER_PROTOCOL_VERSION,
            body: TransferRequestBody::UpdateShareState {
                share_local_history,
            },
        }
    }

    pub fn fetch_files(item_id: Uuid) -> Self {
        Self {
            protocol_version: TRANSFER_PROTOCOL_VERSION,
            body: TransferRequestBody::FetchFiles { item_id },
        }
    }

    pub fn fetch_clipboard_item(item_id: Uuid) -> Self {
        Self {
            protocol_version: TRANSFER_PROTOCOL_VERSION,
            body: TransferRequestBody::FetchClipboardItem { item_id },
        }
    }

    pub fn validate(&self) -> AppResult<()> {
        validate_protocol_version(self.protocol_version)?;

        match &self.body {
            TransferRequestBody::PushClipboard {
                item,
                include_files,
            } => item.validate(*include_files),
            TransferRequestBody::RemoveHistoryItems { item_ids } => {
                if item_ids.is_empty() {
                    return Err(AppError::Network(
                        "History removal request must include at least one item id.".to_string(),
                    ));
                }
                Ok(())
            }
            TransferRequestBody::UpdateShareState { .. } => Ok(()),
            TransferRequestBody::RevokeTrust => Ok(()),
            TransferRequestBody::FetchHistorySnapshot { limit } => {
                if *limit == 0 {
                    return Err(AppError::Network(
                        "History snapshot request limit must be greater than zero.".to_string(),
                    ));
                }
                Ok(())
            }
            TransferRequestBody::FetchClipboardItem { .. } => Ok(()),
            TransferRequestBody::FetchFiles { .. } => Ok(()),
        }
    }
}

impl TransferResponse {
    pub fn ack() -> Self {
        Self {
            protocol_version: TRANSFER_PROTOCOL_VERSION,
            body: TransferResponseBody::Ack,
        }
    }

    pub fn history_snapshot(items: &[ClipboardItem]) -> Self {
        Self {
            protocol_version: TRANSFER_PROTOCOL_VERSION,
            body: TransferResponseBody::HistorySnapshot {
                items: items.iter().map(WireClipboardItem::from_item).collect(),
            },
        }
    }

    pub fn clipboard_item(item: &ClipboardItem, include_files: bool) -> Self {
        Self {
            protocol_version: TRANSFER_PROTOCOL_VERSION,
            body: TransferResponseBody::ClipboardItem {
                item: WireClipboardItem::from_item(item),
                include_files,
            },
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            protocol_version: TRANSFER_PROTOCOL_VERSION,
            body: TransferResponseBody::Error {
                message: message.into(),
            },
        }
    }

    pub fn validate(&self) -> AppResult<()> {
        validate_protocol_version(self.protocol_version)?;

        match &self.body {
            TransferResponseBody::Ack => Ok(()),
            TransferResponseBody::HistorySnapshot { items } => {
                for item in items {
                    item.validate(false)?;
                }
                Ok(())
            }
            TransferResponseBody::ClipboardItem {
                item,
                include_files,
            } => item.validate(*include_files),
            TransferResponseBody::Error { message } => {
                if message.trim().is_empty() {
                    return Err(AppError::Network(
                        "Transfer error response cannot be empty.".to_string(),
                    ));
                }
                Ok(())
            }
        }
    }
}

impl WireClipboardItem {
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
            id: item.id,
            signature: item.signature.clone(),
            kind: item.kind.clone(),
            summary: item.summary.clone(),
            text,
            files,
            source_device_id: item.source_device_id.clone(),
            source_device_name: item.source_device_name.clone(),
            created_at: item.created_at,
        }
    }

    pub fn validate(&self, include_files: bool) -> AppResult<()> {
        match self.kind {
            ClipboardKind::Text => {
                if self.text.is_none() {
                    return Err(AppError::Network(
                        "Text clipboard payload is missing inline text.".to_string(),
                    ));
                }
                if !self.files.is_empty() {
                    return Err(AppError::Network(
                        "Text clipboard payload cannot contain file descriptors.".to_string(),
                    ));
                }
                if include_files {
                    return Err(AppError::Network(
                        "Text clipboard payload cannot include streamed file bytes.".to_string(),
                    ));
                }
            }
            ClipboardKind::Files => {
                if self.text.is_some() {
                    return Err(AppError::Network(
                        "File clipboard payload cannot contain inline text.".to_string(),
                    ));
                }
                if self.files.is_empty() {
                    return Err(AppError::Network(
                        "File clipboard payload is missing file descriptors.".to_string(),
                    ));
                }
            }
        }

        Ok(())
    }

    pub fn into_metadata_item(self) -> AppResult<ClipboardItem> {
        self.validate(false)?;
        let Self {
            id,
            signature,
            kind,
            summary,
            text,
            files,
            source_device_id,
            source_device_name,
            created_at,
        } = self;

        let payload = match kind {
            ClipboardKind::Text => ClipboardPayload::Text(text.unwrap_or_default()),
            ClipboardKind::Files => ClipboardPayload::Files(
                files
                    .into_iter()
                    .map(|file| FileDescriptor {
                        name: file.name,
                        relative_path: file.relative_path,
                        size_bytes: file.size_bytes,
                        source_path: None,
                        local_path: None,
                    })
                    .collect(),
            ),
        };

        Ok(WireClipboardItem {
            id,
            signature,
            kind,
            summary,
            text: None,
            files: Vec::new(),
            source_device_id,
            source_device_name,
            created_at,
        }
        .into_item(payload))
    }

    pub fn into_item_with_local_files(self, local_files: Vec<PathBuf>) -> AppResult<ClipboardItem> {
        self.validate(true)?;
        let Self {
            id,
            signature,
            kind,
            summary,
            text,
            files,
            source_device_id,
            source_device_name,
            created_at,
        } = self;

        let payload = match kind {
            ClipboardKind::Text => {
                if !local_files.is_empty() {
                    return Err(AppError::Network(
                        "Text clipboard payload unexpectedly produced local files.".to_string(),
                    ));
                }
                ClipboardPayload::Text(text.unwrap_or_default())
            }
            ClipboardKind::Files => {
                if files.len() != local_files.len() {
                    return Err(AppError::Network(format!(
                        "File clipboard payload expected {} files, received {} files.",
                        files.len(),
                        local_files.len()
                    )));
                }

                ClipboardPayload::Files(
                    files
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
                )
            }
        };

        Ok(WireClipboardItem {
            id,
            signature,
            kind,
            summary,
            text: None,
            files: Vec::new(),
            source_device_id,
            source_device_name,
            created_at,
        }
        .into_item(payload))
    }

    fn into_item(self, payload: ClipboardPayload) -> ClipboardItem {
        ClipboardItem {
            id: self.id,
            kind: self.kind,
            summary: self.summary,
            signature: self.signature,
            payload,
            source_device_id: self.source_device_id,
            source_device_name: self.source_device_name,
            created_at: self.created_at,
            is_remote: true,
            is_pinned: false,
        }
    }
}

fn validate_protocol_version(protocol_version: u8) -> AppResult<()> {
    if protocol_version != TRANSFER_PROTOCOL_VERSION {
        return Err(AppError::Network(format!(
            "Unsupported transfer protocol version {}.",
            protocol_version
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;

    use super::{
        TRANSFER_PROTOCOL_VERSION, TransferRequest, TransferRequestBody, TransferResponse,
        TransferResponseBody, WireClipboardItem, WireFile,
    };
    use crate::core::model::{ClipboardItem, ClipboardKind, ClipboardPayload};
    use uuid::Uuid;

    fn sample_text_item() -> ClipboardItem {
        ClipboardItem {
            id: Uuid::new_v4(),
            signature: "sig-text".to_string(),
            kind: ClipboardKind::Text,
            summary: "text".to_string(),
            payload: ClipboardPayload::Text("hello".to_string()),
            source_device_id: Some("device-a".to_string()),
            source_device_name: Some("Device A".to_string()),
            created_at: OffsetDateTime::now_utc(),
            is_remote: false,
            is_pinned: false,
        }
    }

    fn sample_file_wire_item() -> WireClipboardItem {
        WireClipboardItem {
            id: Uuid::new_v4(),
            signature: "sig-files".to_string(),
            kind: ClipboardKind::Files,
            summary: "file".to_string(),
            text: None,
            files: vec![WireFile {
                name: "a.txt".to_string(),
                relative_path: "a.txt".to_string(),
                size_bytes: 4,
            }],
            source_device_id: Some("device-a".to_string()),
            source_device_name: Some("Device A".to_string()),
            created_at: OffsetDateTime::now_utc(),
        }
    }

    fn sample_file_item() -> ClipboardItem {
        sample_file_wire_item()
            .into_metadata_item()
            .expect("metadata file item should be valid")
    }

    #[test]
    fn unsupported_protocol_version_is_rejected() {
        let mut request = TransferRequest::fetch_history_snapshot(8);
        request.protocol_version = TRANSFER_PROTOCOL_VERSION + 1;
        assert!(request.validate().is_err());
    }

    #[test]
    fn remove_history_items_requires_at_least_one_item() {
        let request = TransferRequest::remove_history_items(&[]);
        assert!(request.validate().is_err());
    }

    #[test]
    fn file_payload_requires_matching_local_file_count() {
        let item = sample_file_wire_item();
        let error = item.into_item_with_local_files(Vec::new()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("expected 1 files, received 0 files")
        );
    }

    #[test]
    fn text_payload_rejects_embedded_file_descriptors() {
        let mut request = TransferRequest::push_clipboard(&sample_text_item());
        if let TransferRequestBody::PushClipboard { item, .. } = &mut request.body {
            item.files.push(WireFile {
                name: "unexpected.txt".to_string(),
                relative_path: "unexpected.txt".to_string(),
                size_bytes: 1,
            });
        }
        assert!(request.validate().is_err());
    }

    #[test]
    fn metadata_only_file_item_roundtrip_preserves_remote_file_descriptors() {
        let item = sample_file_wire_item().into_metadata_item().unwrap();
        assert!(item.is_remote);
        assert!(matches!(item.payload, ClipboardPayload::Files(_)));
        let ClipboardPayload::Files(files) = item.payload else {
            unreachable!("validated above");
        };
        assert_eq!(files.len(), 1);
        assert!(files[0].local_path.is_none());
        assert!(files[0].source_path.is_none());
    }

    #[test]
    fn push_clipboard_keeps_file_items_metadata_only() {
        let request = TransferRequest::push_clipboard(&sample_file_item());
        let TransferRequestBody::PushClipboard { include_files, .. } = request.body else {
            unreachable!("push clipboard must produce push request");
        };
        assert!(!include_files);
    }

    #[test]
    fn history_snapshot_response_rejects_invalid_text_file_bytes() {
        let mut response = TransferResponse {
            protocol_version: TRANSFER_PROTOCOL_VERSION,
            body: TransferResponseBody::ClipboardItem {
                item: WireClipboardItem::from_item(&sample_text_item()),
                include_files: true,
            },
        };
        assert!(response.validate().is_err());

        response.body = TransferResponseBody::HistorySnapshot {
            items: vec![WireClipboardItem::from_item(&sample_text_item())],
        };
        assert!(response.validate().is_ok());
    }
}
