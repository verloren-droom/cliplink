use std::{
    fs,
    path::{Component, Path, PathBuf},
    time::Duration,
};

use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    constants::timing::CLIPBOARD_POLL_INTERVAL,
    core::{
        error::AppResult,
        model::{ClipboardItem, ClipboardKind, ClipboardPayload, FileDescriptor},
    },
};

pub trait ClipboardBackend: Send + Sync {
    fn recommended_poll_interval(&self) -> Duration {
        CLIPBOARD_POLL_INTERVAL
    }

    fn poll(
        &self,
        source_device_id: &str,
        source_device_name: &str,
    ) -> AppResult<Option<ClipboardItem>>;

    fn write_item(&self, item: &ClipboardItem) -> AppResult<()>;
}

pub fn build_item_from_text(
    text: &str,
    source_device_id: Option<String>,
    source_device_name: Option<String>,
    is_remote: bool,
) -> ClipboardItem {
    if let Some(files) = detect_file_descriptors(text) {
        build_item_from_descriptors(files, source_device_id, source_device_name, is_remote)
    } else {
        ClipboardItem {
            id: Uuid::new_v4(),
            kind: ClipboardKind::Text,
            summary: summarize_text(text),
            signature: hash_strings("text", &[text.to_string()]),
            payload: ClipboardPayload::Text(text.to_string()),
            source_device_id,
            source_device_name,
            created_at: OffsetDateTime::now_utc(),
            is_remote,
            is_pinned: false,
        }
    }
}

#[cfg_attr(target_os = "android", allow(dead_code))]
pub fn build_item_from_paths(
    paths: &[PathBuf],
    source_device_id: Option<String>,
    source_device_name: Option<String>,
    is_remote: bool,
) -> Option<ClipboardItem> {
    let files = collect_file_descriptors(paths)?;
    Some(build_item_from_descriptors(
        files,
        source_device_id,
        source_device_name,
        is_remote,
    ))
}

fn detect_file_descriptors(text: &str) -> Option<Vec<FileDescriptor>> {
    let candidates = text
        .lines()
        .map(parse_candidate_path)
        .collect::<Option<Vec<_>>>()?;
    collect_file_descriptors(&candidates)
}

fn collect_file_descriptors(paths: &[PathBuf]) -> Option<Vec<FileDescriptor>> {
    if paths.is_empty() {
        return None;
    }

    let mut files = Vec::new();
    for candidate in paths {
        if !candidate.exists() {
            return None;
        }
        if candidate.is_dir() {
            collect_directory_files(candidate, candidate, &mut files);
        } else if candidate.is_file() {
            files.push(descriptor_from_path(
                candidate,
                candidate.file_name()?.to_string_lossy().as_ref(),
            ));
        } else {
            return None;
        }
    }

    if files.is_empty() { None } else { Some(files) }
}

fn build_item_from_descriptors(
    files: Vec<FileDescriptor>,
    source_device_id: Option<String>,
    source_device_name: Option<String>,
    is_remote: bool,
) -> ClipboardItem {
    let summary = summarize_files(&files);
    let signature = hash_strings(
        "files",
        &files
            .iter()
            .map(|file| format!("{}:{}", file.relative_path, file.size_bytes))
            .collect::<Vec<_>>(),
    );

    ClipboardItem {
        id: Uuid::new_v4(),
        kind: ClipboardKind::Files,
        summary,
        signature,
        payload: ClipboardPayload::Files(files),
        source_device_id,
        source_device_name,
        created_at: OffsetDateTime::now_utc(),
        is_remote,
        is_pinned: false,
    }
}

fn parse_candidate_path(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(path) = trimmed.strip_prefix("file://") {
        let candidate = PathBuf::from(path);
        return candidate.is_absolute().then_some(candidate);
    }

    let candidate = PathBuf::from(trimmed);
    candidate.is_absolute().then_some(candidate)
}

fn collect_directory_files(root: &Path, current: &Path, files: &mut Vec<FileDescriptor>) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_directory_files(root, &path, files);
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root.parent().unwrap_or(root))
                .ok()
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            files.push(descriptor_from_path(&path, &relative));
        }
    }
}

fn descriptor_from_path(path: &Path, relative: &str) -> FileDescriptor {
    let size_bytes = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    FileDescriptor {
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| relative.to_string()),
        relative_path: sanitize_relative_path(relative),
        size_bytes,
        source_path: Some(path.to_path_buf()),
        local_path: Some(path.to_path_buf()),
    }
}

pub fn sanitize_relative_path(path: &str) -> String {
    let mut clean = PathBuf::new();
    for component in Path::new(path).components() {
        if let Component::Normal(part) = component {
            clean.push(part);
        }
    }
    clean.to_string_lossy().replace('\\', "/")
}

fn summarize_text(text: &str) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(text)
        .chars()
        .take(80)
        .collect()
}

fn summarize_files(files: &[FileDescriptor]) -> String {
    match files.len() {
        0 => "无文件".to_string(),
        1 => files[0].relative_path.clone(),
        count => format!("{count} 个文件 · {}", files[0].relative_path),
    }
}

fn hash_strings(prefix: &str, values: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prefix.as_bytes());
    for value in values {
        hasher.update([0]);
        hasher.update(value.as_bytes());
    }
    let hash = hasher.finalize();
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}
