package com.benfach.cliplink

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import androidx.core.content.FileProvider
import java.io.File
import java.util.concurrent.TimeUnit

object ClipboardInterop {
    fun submitCurrentClipboard(
        context: Context,
        clipboardManager: ClipboardManager,
    ): String? {
        val clip = clipboardManager.primaryClip ?: return null
        val payload = extractClipboardPayload(context, clip) ?: return null
        return RustBridge.nativeSubmitClipboardText(payload)
    }

    fun applyPendingClipboard(
        context: Context,
        clipboardManager: ClipboardManager,
        payload: RustBridge.PendingClipboard?,
    ) {
        if (payload == null) {
            return
        }

        when {
            payload.kind == "files" && payload.paths.isNotEmpty() -> {
                applyPendingFileClipboard(context, clipboardManager, payload)
            }

            !payload.text.isNullOrEmpty() -> {
                val clip = ClipData.newPlainText("cliplink", payload.text)
                clipboardManager.setPrimaryClip(clip)
            }
        }
    }

    private fun applyPendingFileClipboard(
        context: Context,
        clipboardManager: ClipboardManager,
        payload: RustBridge.PendingClipboard,
    ) {
        val uris = payload.paths.mapNotNull { rawPath ->
            val file = File(rawPath)
            if (!file.exists()) {
                return@mapNotNull null
            }

            runCatching {
                FileProvider.getUriForFile(
                    context,
                    "${context.applicationContext.packageName}.fileprovider",
                    file,
                )
            }.getOrNull()
        }

        if (uris.isEmpty()) {
            if (!payload.text.isNullOrEmpty()) {
                val clip = ClipData.newPlainText("cliplink", payload.text)
                clipboardManager.setPrimaryClip(clip)
            }
            return
        }

        val clip = ClipData.newUri(
            context.contentResolver,
            "cliplink-files",
            uris.first(),
        )
        uris.drop(1).forEach { uri ->
            clip.addItem(ClipData.Item(uri))
        }
        clipboardManager.setPrimaryClip(clip)
    }

    private fun extractClipboardPayload(
        context: Context,
        clip: ClipData,
    ): String? {
        if (clip.itemCount == 0) {
            return null
        }

        var firstText: String? = null
        val filePaths = mutableListOf<String>()
        for (index in 0 until clip.itemCount) {
            val item = clip.getItemAt(index)
            val uriPath = item.uri?.let { resolveUriToPath(context, it) }
            if (!uriPath.isNullOrBlank()) {
                filePaths += uriPath
            }

            if (firstText.isNullOrBlank()) {
                val text = item.coerceToText(context)?.toString().orEmpty().trim()
                if (text.isNotEmpty()) {
                    firstText = text
                }
            }
        }

        return if (filePaths.isNotEmpty()) {
            filePaths.joinToString("\n")
        } else {
            firstText
        }
    }

    private fun resolveUriToPath(
        context: Context,
        uri: Uri,
    ): String? {
        return when (uri.scheme?.lowercase()) {
            "file" -> uri.path?.takeIf { it.isNotBlank() }
            "content" -> copyContentUriToCache(context, uri)
            else -> null
        }
    }

    private fun copyContentUriToCache(
        context: Context,
        uri: Uri,
    ): String? {
        return runCatching {
            pruneImportCache(context)
            val displayName = queryDisplayName(context, uri)?.ifBlank { null } ?: "shared"
            val safeName = displayName.replace(Regex("[^A-Za-z0-9._-]"), "_")
            val importDir = File(context.cacheDir, "cliplink-imports").apply { mkdirs() }
            val target = File(importDir, "${System.currentTimeMillis()}_$safeName")
            val input = context.contentResolver.openInputStream(uri) ?: return@runCatching null
            input.use { source ->
                target.outputStream().use { output ->
                    source.copyTo(output)
                }
            }
            target.absolutePath
        }.getOrNull()
    }

    private fun pruneImportCache(context: Context) {
        val importDir = File(context.cacheDir, "cliplink-imports")
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
        return runCatching {
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
        }.getOrNull()
    }
}
