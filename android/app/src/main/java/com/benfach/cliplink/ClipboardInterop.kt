package com.benfach.cliplink

import android.content.ClipData
import android.content.ClipboardManager
import android.content.ContentValues
import android.content.Context
import android.net.Uri
import android.os.Environment
import android.provider.MediaStore
import android.provider.OpenableColumns
import androidx.core.content.FileProvider
import java.io.File
import java.net.URLConnection
import java.util.concurrent.TimeUnit

object ClipboardInterop {
    private const val CLIPLINK_TEXT_LABEL = "cliplink"
    private const val CLIPLINK_FILES_LABEL = "cliplink-files"
    private const val IMPORT_DIRECTORY_NAME = "cliplink/imported"
    private const val REMOTE_INBOX_DIRECTORY_NAME = "cliplink/incoming"

    @Volatile
    private var suppressNextClipboardSubmission: Boolean = false

    @Volatile
    private var lastSubmittedSnapshotKey: String? = null

    private data class ClipboardExtractionResult(
        val text: String?,
        val filePaths: List<String>,
        val error: String?,
    )

    data class ClipboardSubmissionResult(
        val submitted: Boolean,
        val error: String?,
        val requiresForegroundAccess: Boolean = false,
    )

    private data class PathResolutionResult(
        val path: String?,
        val error: String?,
    )

    private data class ClipboardUriResult(
        val uri: Uri,
        val downloadedFile: DownloadHistoryStore.DownloadedFile?,
    )

    fun submitCurrentClipboard(
        context: Context,
        clipboardManager: ClipboardManager,
    ): ClipboardSubmissionResult {
        val clip = try {
            clipboardManager.primaryClip
        } catch (_: SecurityException) {
            return ClipboardSubmissionResult(
                submitted = false,
                error = null,
                requiresForegroundAccess = true,
            )
        } catch (_: Throwable) {
            return ClipboardSubmissionResult(
                submitted = false,
                error = context.getString(R.string.clipboard_content_unreadable),
            )
        } ?: return ClipboardSubmissionResult(submitted = false, error = null)
        if (isManagedClip(context, clip)) {
            consumeSuppressedPayload()
            lastSubmittedSnapshotKey = null
            return ClipboardSubmissionResult(submitted = false, error = null)
        }
        val extraction = extractClipboardPayload(context, clip)
        if (extraction.error != null) {
            return ClipboardSubmissionResult(submitted = false, error = extraction.error)
        }
        val snapshotKey = submissionSnapshotKey(extraction.text, extraction.filePaths)
            ?: return ClipboardSubmissionResult(submitted = false, error = null)
        if (consumeSuppressedPayload()) {
            lastSubmittedSnapshotKey = snapshotKey
            return ClipboardSubmissionResult(submitted = false, error = null)
        }
        if (snapshotKey == lastSubmittedSnapshotKey) {
            return ClipboardSubmissionResult(submitted = false, error = null)
        }
        val error = when {
            extraction.filePaths.isNotEmpty() -> RustBridge.submitClipboardFiles(extraction.filePaths)
            !extraction.text.isNullOrBlank() -> RustBridge.nativeSubmitClipboardText(
                extraction.text.orEmpty(),
            )
            else -> null
        }
        if (error == null) {
            lastSubmittedSnapshotKey = snapshotKey
            return ClipboardSubmissionResult(submitted = true, error = null)
        }
        return ClipboardSubmissionResult(submitted = false, error = error)
    }

    fun applyPendingClipboard(
        context: Context,
        clipboardManager: ClipboardManager,
        payload: RustBridge.PendingClipboard?,
    ): Boolean {
        if (payload == null) {
            return false
        }

        when {
            payload.kind == "files" -> {
                if (applyPendingFileClipboard(context, clipboardManager, payload)) {
                    rememberSuppressedPayload(payload)
                    return true
                }
            }

            !payload.text.isNullOrEmpty() -> {
                val clip = ClipData.newPlainText(CLIPLINK_TEXT_LABEL, payload.text)
                rememberSuppressedPayload(payload)
                clipboardManager.setPrimaryClip(clip)
                return true
            }
        }

        return false
    }

    private fun rememberSuppressedPayload(@Suppress("UNUSED_PARAMETER") payload: RustBridge.PendingClipboard) {
        suppressNextClipboardSubmission = true
    }

    private fun consumeSuppressedPayload(): Boolean {
        if (!suppressNextClipboardSubmission) {
            return false
        }
        suppressNextClipboardSubmission = false
        return true
    }

    private fun applyPendingFileClipboard(
        context: Context,
        clipboardManager: ClipboardManager,
        payload: RustBridge.PendingClipboard,
    ): Boolean {
        if (payload.paths.isEmpty()) {
            return false
        }

        val resolvedUris = payload.paths.mapNotNull { rawPath ->
            buildClipboardUriForPath(context, rawPath)
        }
        val uris = resolvedUris.map { it.uri }

        if (uris.isEmpty()) {
            return false
        }

        val clip = ClipData.newUri(
            context.contentResolver,
            CLIPLINK_FILES_LABEL,
            uris.first(),
        )
        uris.drop(1).forEach { uri ->
            clip.addItem(ClipData.Item(uri))
        }
        clipboardManager.setPrimaryClip(clip)

        val downloadedFiles = resolvedUris
            .mapNotNull { result -> result.downloadedFile }
        if (downloadedFiles.isNotEmpty()) {
            DownloadHistoryStore.recordDownload(context, payload, downloadedFiles)
        }
        return true
    }

    private fun submissionSnapshotKey(text: String?, filePaths: List<String>): String? {
        return when {
            filePaths.isNotEmpty() -> "files:${filePaths.joinToString(separator = "\u001F")}"
            !text.isNullOrBlank() -> "text:$text"
            else -> null
        }
    }

    private fun buildClipboardUriForPath(context: Context, rawPath: String): ClipboardUriResult? {
        val file = File(rawPath)
        if (!file.exists()) {
            return null
        }

        if (shouldExportToDownloads(context, file)) {
            val exported = exportFileToDownloads(file, context)
            if (exported != null) {
                return ClipboardUriResult(
                    uri = Uri.parse(exported.uri),
                    downloadedFile = exported,
                )
            }
        }

        val fileUri = fileProviderUri(context, file) ?: return null
        return ClipboardUriResult(uri = fileUri, downloadedFile = null)
    }

    private fun shouldExportToDownloads(context: Context, file: File): Boolean {
        val inboxRoot = File(context.filesDir, REMOTE_INBOX_DIRECTORY_NAME)
        return runCatching {
            val path = file.canonicalPath
            val root = inboxRoot.canonicalPath
            path == root || path.startsWith("$root${File.separator}")
        }.getOrDefault(false)
    }

    private fun exportFileToDownloads(
        source: File,
        context: Context,
    ): DownloadHistoryStore.DownloadedFile? {
        val resolver = context.contentResolver
        val displayName = buildExportDisplayName(source.name)
        val values = ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, displayName)
            put(MediaStore.MediaColumns.MIME_TYPE, guessMimeType(displayName))
            put(MediaStore.MediaColumns.RELATIVE_PATH, downloadRelativePath())
            put(MediaStore.MediaColumns.IS_PENDING, 1)
        }

        val targetUri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
            ?: return null

        return try {
            resolver.openOutputStream(targetUri, "w")?.use { output ->
                source.inputStream().use { input ->
                    input.copyTo(output)
                }
            } ?: throw IllegalStateException("Failed to open output stream for downloads export")

            val publishValues = ContentValues().apply {
                put(MediaStore.MediaColumns.IS_PENDING, 0)
            }
            resolver.update(targetUri, publishValues, null, null)
            DownloadHistoryStore.DownloadedFile(
                displayName = displayName,
                uri = targetUri.toString(),
                sizeBytes = source.length().coerceAtLeast(0L),
            )
        } catch (_: Throwable) {
            resolver.delete(targetUri, null, null)
            null
        }
    }

    private fun buildExportDisplayName(rawName: String): String {
        val normalized = rawName.ifBlank { "cliplink_file" }
        return "${System.currentTimeMillis()}_$normalized"
    }

    private fun downloadRelativePath(): String {
        return "${Environment.DIRECTORY_DOWNLOADS}/ClipLink"
    }

    private fun guessMimeType(fileName: String): String {
        return URLConnection.guessContentTypeFromName(fileName) ?: "application/octet-stream"
    }

    private fun fileProviderUri(context: Context, file: File): Uri? {
        return runCatching {
            FileProvider.getUriForFile(
                context,
                "${context.applicationContext.packageName}.fileprovider",
                file,
            )
        }.getOrNull()
    }

    private fun extractClipboardPayload(
        context: Context,
        clip: ClipData,
    ): ClipboardExtractionResult {
        if (clip.itemCount == 0) {
            return ClipboardExtractionResult(text = null, filePaths = emptyList(), error = null)
        }

        var firstText: String? = null
        val filePaths = mutableListOf<String>()
        for (index in 0 until clip.itemCount) {
            val item = clip.getItemAt(index)
            val uriResolution = item.uri?.let { resolveUriToPath(context, it) }
            if (uriResolution != null) {
                if (uriResolution.error != null) {
                    return ClipboardExtractionResult(
                        text = null,
                        filePaths = emptyList(),
                        error = uriResolution.error,
                    )
                }
                if (!uriResolution.path.isNullOrBlank()) {
                    filePaths += uriResolution.path
                }
            }

            if (firstText.isNullOrBlank()) {
                val text = item.coerceToText(context)?.toString().orEmpty().trim()
                if (text.isNotEmpty()) {
                    firstText = text
                }
            }
        }

        return ClipboardExtractionResult(text = firstText, filePaths = filePaths, error = null)
    }

    private fun resolveUriToPath(
        context: Context,
        uri: Uri,
    ): PathResolutionResult {
        if (uri.authority == managedFileProviderAuthority(context)) {
            return PathResolutionResult(path = null, error = null)
        }

        return when (uri.scheme?.lowercase()) {
            "file" -> PathResolutionResult(
                path = uri.path?.takeIf { it.isNotBlank() },
                error = null,
            )
            "content" -> copyContentUriToManagedFiles(context, uri)
            else -> PathResolutionResult(path = null, error = null)
        }
    }

    private fun copyContentUriToManagedFiles(
        context: Context,
        uri: Uri,
    ): PathResolutionResult {
        return try {
            pruneImportedFiles(context)
            val displayName = queryDisplayName(context, uri)?.ifBlank { null } ?: "shared"
            val safeName = displayName.replace(Regex("[^A-Za-z0-9._-]"), "_")
            val importDir = managedImportDirectory(context).apply { mkdirs() }
            val target = File(importDir, "${System.currentTimeMillis()}_$safeName")
            val input = context.contentResolver.openInputStream(uri)
                ?: return PathResolutionResult(
                    path = null,
                    error = context.getString(R.string.clipboard_content_unreadable),
                )
            input.use { source ->
                target.outputStream().use { output ->
                    source.copyTo(output)
                }
            }
            PathResolutionResult(path = target.absolutePath, error = null)
        } catch (_: SecurityException) {
            PathResolutionResult(
                path = null,
                error = context.getString(R.string.clipboard_permission_required),
            )
        } catch (_: Throwable) {
            PathResolutionResult(
                path = null,
                error = context.getString(R.string.clipboard_content_unreadable),
            )
        }
    }

    private fun pruneImportedFiles(context: Context) {
        val importDir = managedImportDirectory(context)
        if (!importDir.isDirectory) {
            return
        }

        val now = System.currentTimeMillis()
        val retentionMillis = TimeUnit.DAYS.toMillis(2)
        importDir.listFiles()?.forEach { file ->
            if (now - file.lastModified() > retentionMillis) {
                file.delete()
            }
        }
    }

    private fun queryDisplayName(
        context: Context,
        uri: Uri,
    ): String? {
        return try {
            context.contentResolver.query(
                uri,
                arrayOf(OpenableColumns.DISPLAY_NAME),
                null,
                null,
                null,
            )?.use { cursor ->
                val column = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                if (column >= 0 && cursor.moveToFirst()) {
                    cursor.getString(column)
                } else {
                    null
                }
            }
        } catch (_: Throwable) {
            null
        }
    }

    private fun isManagedClip(
        context: Context,
        clip: ClipData,
    ): Boolean {
        val label = clip.description.label?.toString().orEmpty()
        if (label == CLIPLINK_TEXT_LABEL || label == CLIPLINK_FILES_LABEL) {
            return true
        }

        val authority = managedFileProviderAuthority(context)
        for (index in 0 until clip.itemCount) {
            if (clip.getItemAt(index).uri?.authority == authority) {
                return true
            }
        }
        return false
    }

    private fun managedFileProviderAuthority(context: Context): String {
        return "${context.applicationContext.packageName}.fileprovider"
    }

    private fun managedImportDirectory(context: Context): File {
        return File(context.filesDir, IMPORT_DIRECTORY_NAME)
    }
}
