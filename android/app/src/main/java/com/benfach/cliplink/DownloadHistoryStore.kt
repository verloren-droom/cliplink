package com.benfach.cliplink

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import org.json.JSONArray
import org.json.JSONObject

object DownloadHistoryStore {
    private const val PREFS_NAME = "cliplink_download_history"
    private const val PREFS_KEY_RECORDS = "records"
    private const val MAX_RECORDS = 24
    private const val DOWNLOAD_FOLDER_DISPLAY = "Download/ClipLink"

    data class DownloadedFile(
        val displayName: String,
        val uri: String,
        val sizeBytes: Long,
    )

    data class DownloadRecord(
        val recordId: String,
        val itemId: String?,
        val title: String,
        val sourceDeviceName: String,
        val folderDisplay: String,
        val totalBytes: Long,
        val fileCount: Int,
        val files: List<DownloadedFile>,
        val createdAtEpochMs: Long,
    )

    data class RemovalResult(
        val removed: Boolean,
        val deletedFiles: Int,
        val failedDeletes: Int,
    )

    fun downloadFolderDisplay(): String = DOWNLOAD_FOLDER_DISPLAY

    fun listRecords(context: Context): List<DownloadRecord> {
        val appContext = context.applicationContext
        val raw = appContext
            .getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            .getString(PREFS_KEY_RECORDS, null)
            ?: return emptyList()
        val parsed = runCatching {
            val records = JSONArray(raw)
            buildList(records.length()) {
                for (index in 0 until records.length()) {
                    val record = records.optJSONObject(index) ?: continue
                    val parsed = record.toDownloadRecordOrNull() ?: continue
                    add(parsed)
                }
            }
        }.getOrDefault(emptyList())
        val reconciled = reconcileRecords(appContext, parsed)
        if (reconciled != parsed) {
            persist(appContext, reconciled)
        }
        return reconciled
    }

    fun recordDownload(
        context: Context,
        payload: RustBridge.PendingClipboard,
        files: List<DownloadedFile>,
    ) {
        if (files.isEmpty()) {
            return
        }

        val now = System.currentTimeMillis()
        val recordId = payload.itemId?.takeIf { it.isNotBlank() } ?: "download-$now"
        val record = DownloadRecord(
            recordId = recordId,
            itemId = payload.itemId?.takeIf { it.isNotBlank() },
            title = payload.summary
                ?.takeIf { it.isNotBlank() }
                ?: defaultRecordTitle(files),
            sourceDeviceName = payload.sourceDeviceName
                ?.takeIf { it.isNotBlank() }
                ?: "远端设备",
            folderDisplay = DOWNLOAD_FOLDER_DISPLAY,
            totalBytes = files.sumOf { it.sizeBytes.coerceAtLeast(0L) },
            fileCount = files.size,
            files = files,
            createdAtEpochMs = now,
        )

        val records = listRecords(context)
            .filterNot { existing ->
                existing.recordId == record.recordId ||
                    (!record.itemId.isNullOrBlank() && existing.itemId == record.itemId)
            }
            .toMutableList()
        records.add(0, record)
        persist(context, records.take(MAX_RECORDS))
    }

    fun removeRecord(
        context: Context,
        recordId: String,
        deleteFiles: Boolean,
    ): RemovalResult {
        val records = listRecords(context)
        val record = records.firstOrNull { it.recordId == recordId } ?: return RemovalResult(
            removed = false,
            deletedFiles = 0,
            failedDeletes = 0,
        )

        var deletedFiles = 0
        var failedDeletes = 0
        if (deleteFiles) {
            for (file in record.files) {
                val removed = runCatching {
                    context.contentResolver.delete(Uri.parse(file.uri), null, null) > 0
                }.getOrDefault(false)
                if (removed) {
                    deletedFiles += 1
                } else {
                    failedDeletes += 1
                }
            }
        }

        persist(context, records.filterNot { it.recordId == recordId })
        return RemovalResult(
            removed = true,
            deletedFiles = deletedFiles,
            failedDeletes = failedDeletes,
        )
    }

    private fun defaultRecordTitle(files: List<DownloadedFile>): String {
        if (files.isEmpty()) {
            return "已下载文件"
        }
        if (files.size == 1) {
            return files.first().displayName
        }
        return "${files.size} 个文件 · ${files.first().displayName}"
    }

    private fun reconcileRecords(
        context: Context,
        records: List<DownloadRecord>,
    ): List<DownloadRecord> {
        return buildList(records.size) {
            records.forEach { record ->
                reconcileRecord(context, record)?.let(::add)
            }
        }
    }

    private fun reconcileRecord(
        context: Context,
        record: DownloadRecord,
    ): DownloadRecord? {
        val existingFiles = record.files.mapNotNull { file ->
            resolveExistingFile(context, file)
        }
        if (existingFiles.isEmpty()) {
            return null
        }

        val normalizedTitle = when {
            record.title.isBlank() -> defaultRecordTitle(existingFiles)
            record.title == defaultRecordTitle(record.files) -> defaultRecordTitle(existingFiles)
            else -> record.title
        }

        return record.copy(
            title = normalizedTitle,
            totalBytes = existingFiles.sumOf { it.sizeBytes.coerceAtLeast(0L) },
            fileCount = existingFiles.size,
            files = existingFiles,
        )
    }

    private fun resolveExistingFile(
        context: Context,
        file: DownloadedFile,
    ): DownloadedFile? {
        val uri = runCatching { Uri.parse(file.uri) }.getOrNull() ?: return null
        return runCatching {
            context.contentResolver.query(
                uri,
                arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE),
                null,
                null,
                null,
            )?.use { cursor ->
                if (!cursor.moveToFirst()) {
                    return@use null
                }
                val displayNameColumn = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                val sizeColumn = cursor.getColumnIndex(OpenableColumns.SIZE)
                val displayName = if (displayNameColumn >= 0 && !cursor.isNull(displayNameColumn)) {
                    cursor.getString(displayNameColumn)
                } else {
                    file.displayName
                }
                val sizeBytes = if (sizeColumn >= 0 && !cursor.isNull(sizeColumn)) {
                    cursor.getLong(sizeColumn)
                } else {
                    file.sizeBytes
                }
                DownloadedFile(
                    displayName = displayName,
                    uri = file.uri,
                    sizeBytes = sizeBytes.coerceAtLeast(0L),
                )
            }
        }.getOrNull()
    }

    private fun persist(context: Context, records: List<DownloadRecord>) {
        val payload = JSONArray()
        records.forEach { record ->
            payload.put(
                JSONObject()
                    .put("record_id", record.recordId)
                    .put("item_id", record.itemId)
                    .put("title", record.title)
                    .put("source_device_name", record.sourceDeviceName)
                    .put("folder_display", record.folderDisplay)
                    .put("total_bytes", record.totalBytes)
                    .put("file_count", record.fileCount)
                    .put("created_at_epoch_ms", record.createdAtEpochMs)
                    .put(
                        "files",
                        JSONArray().apply {
                            record.files.forEach { file ->
                                put(
                                    JSONObject()
                                        .put("display_name", file.displayName)
                                        .put("uri", file.uri)
                                        .put("size_bytes", file.sizeBytes),
                                )
                            }
                        },
                    ),
            )
        }
        context.applicationContext
            .getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            .edit()
            .putString(PREFS_KEY_RECORDS, payload.toString())
            .apply()
    }

    private fun JSONObject.toDownloadRecordOrNull(): DownloadRecord? {
        val filesArray = optJSONArray("files")
        val files = buildList(filesArray?.length() ?: 0) {
            if (filesArray == null) {
                return@buildList
            }
            for (index in 0 until filesArray.length()) {
                val file = filesArray.optJSONObject(index) ?: continue
                add(
                    DownloadedFile(
                        displayName = file.optString("display_name"),
                        uri = file.optString("uri"),
                        sizeBytes = file.optLong("size_bytes").coerceAtLeast(0L),
                    ),
                )
            }
        }

        val title = optString("title").ifBlank { defaultRecordTitle(files) }
        val sourceDeviceName = optString("source_device_name").ifBlank { "远端设备" }
        val recordId = optString("record_id")
            .ifBlank { optString("item_id") }
            .ifBlank { null }
        if (recordId == null && title.isBlank() && files.isEmpty()) {
            return null
        }

        return DownloadRecord(
            recordId = recordId ?: "download-${optLong("created_at_epoch_ms").coerceAtLeast(0L)}",
            itemId = optString("item_id").takeIf { it.isNotBlank() },
            title = title,
            sourceDeviceName = sourceDeviceName,
            folderDisplay = optString("folder_display").ifBlank { DOWNLOAD_FOLDER_DISPLAY },
            totalBytes = optLong("total_bytes").coerceAtLeast(0L),
            fileCount = optInt("file_count").coerceAtLeast(files.size),
            files = files,
            createdAtEpochMs = optLong("created_at_epoch_ms").coerceAtLeast(0L),
        )
    }
}
