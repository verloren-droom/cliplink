package com.benfach.cliplink

import android.content.Context
import android.os.Build
import org.json.JSONArray
import org.json.JSONObject

object RustBridge {
    init {
        System.loadLibrary("cliplink")
    }

    data class HistoryItem(
        val id: String,
        val sourceBadge: String,
        val sourceTooltip: String?,
        val summaryText: String,
        val detailTooltip: String?,
        val isRemote: Boolean,
    )

    data class SettingsDevice(
        val deviceId: String,
        val deviceName: String,
        val secondaryText: String,
        val isOnline: Boolean,
        val isTrusted: Boolean,
    )

    data class SettingsSnapshot(
        val deviceName: String,
        val historyLimit: Int,
        val hotkey: String,
        val shareLocalHistory: Boolean,
        val preferRemoteLatestOnPaste: Boolean,
        val discoveryEnabled: Boolean,
        val devices: List<SettingsDevice>,
        val status: String?,
    )

    data class Snapshot(
        val history: List<HistoryItem>,
        val settings: SettingsSnapshot?,
        val pendingClipboard: PendingClipboard?,
        val error: String?,
    )

    data class ActionResult(
        val pendingClipboard: PendingClipboard?,
        val error: String?,
    )

    data class TickResult(
        val pendingClipboard: PendingClipboard?,
        val historyChanged: Boolean,
        val devicesChanged: Boolean,
        val statusChanged: Boolean,
        val error: String?,
    )

    data class PendingClipboard(
        val kind: String,
        val text: String?,
        val paths: List<String>,
    )

    external fun nativeBootstrap(filesDir: String, deviceNameHint: String): String?
    external fun nativeSubmitClipboardText(text: String): String?
    external fun nativeRefreshSnapshot(query: String): String
    external fun nativeTick(): String
    external fun nativeActivateHistoryItem(itemId: String): String
    external fun nativeClearHistory(): String?
    external fun nativeSaveSettings(settingsJson: String): String?
    external fun nativeTrustDevice(deviceId: String): String?
    external fun nativeRevokeTrustedDevice(deviceId: String): String?

    fun bootstrap(context: Context): String? {
        return nativeBootstrap(context.filesDir.absolutePath, deviceNameHint())
    }

    fun refreshSnapshot(query: String): Snapshot {
        return parseSnapshot(nativeRefreshSnapshot(query))
    }

    fun tick(): TickResult {
        val payload = JSONObject(nativeTick())
        return TickResult(
            pendingClipboard = payload.optJSONObject("pending_clipboard")?.toPendingClipboard(),
            historyChanged = payload.optBoolean("history_changed"),
            devicesChanged = payload.optBoolean("devices_changed"),
            statusChanged = payload.optBoolean("status_changed"),
            error = payload.optString("error").takeIf { it.isNotEmpty() },
        )
    }

    fun activateHistoryItem(itemId: String): ActionResult {
        val payload = JSONObject(nativeActivateHistoryItem(itemId))
        return ActionResult(
            pendingClipboard = payload.optJSONObject("pending_clipboard")?.toPendingClipboard()
                ?: payload.optString("clipboard_text")
                    .takeIf { it.isNotEmpty() }
                    ?.let { PendingClipboard(kind = "text", text = it, paths = emptyList()) },
            error = payload.optString("error").takeIf { it.isNotEmpty() },
        )
    }

    fun trustDevice(deviceId: String): String? {
        return nativeTrustDevice(deviceId)
    }

    fun revokeTrustedDevice(deviceId: String): String? {
        return nativeRevokeTrustedDevice(deviceId)
    }

    private fun parseSnapshot(json: String): Snapshot {
        val payload = JSONObject(json)
        val historyItems = payload.optJSONArray("history")?.toHistoryItems().orEmpty()
        val settings = payload.optJSONObject("settings")?.toSettingsSnapshot()
        return Snapshot(
            history = historyItems,
            settings = settings,
            pendingClipboard = payload.optJSONObject("pending_clipboard")?.toPendingClipboard()
                ?: payload.optString("pending_clipboard_text")
                    .takeIf { it.isNotEmpty() }
                    ?.let { PendingClipboard(kind = "text", text = it, paths = emptyList()) },
            error = payload.optString("error").takeIf { it.isNotEmpty() },
        )
    }

    private fun JSONObject.toPendingClipboard(): PendingClipboard {
        val rawPaths = optJSONArray("paths")
        val paths = buildList(rawPaths?.length() ?: 0) {
            if (rawPaths == null) {
                return@buildList
            }
            for (index in 0 until rawPaths.length()) {
                val path = rawPaths.optString(index)
                if (path.isNotEmpty()) {
                    add(path)
                }
            }
        }

        return PendingClipboard(
            kind = optString("kind").ifEmpty { if (paths.isEmpty()) "text" else "files" },
            text = optString("text").takeIf { it.isNotEmpty() },
            paths = paths,
        )
    }

    private fun JSONArray.toHistoryItems(): List<HistoryItem> {
        return buildList(length()) {
            for (index in 0 until length()) {
                val item = getJSONObject(index)
                add(
                    HistoryItem(
                        id = item.getString("id"),
                        sourceBadge = item.optString("source_badge"),
                        sourceTooltip = item.optString("source_tooltip")
                            .ifEmpty { item.optString("source_badge_tooltip") }
                            .takeIf { it.isNotEmpty() },
                        summaryText = item.optString("summary_text")
                            .ifEmpty { item.optString("title") },
                        detailTooltip = item.optString("detail_tooltip")
                            .ifEmpty { item.optString("content_tooltip") }
                            .takeIf { it.isNotEmpty() },
                        isRemote = item.optBoolean("is_remote"),
                    ),
                )
            }
        }
    }

    private fun JSONObject.toSettingsSnapshot(): SettingsSnapshot {
        return SettingsSnapshot(
            deviceName = getString("device_name"),
            historyLimit = getInt("history_limit"),
            hotkey = getString("hotkey"),
            shareLocalHistory = getBoolean("share_local_history"),
            preferRemoteLatestOnPaste = getBoolean("prefer_remote_latest_on_paste"),
            discoveryEnabled = getBoolean("discovery_enabled"),
            devices = optJSONArray("devices")?.let { array ->
                buildList(array.length()) {
                    for (index in 0 until array.length()) {
                        val device = array.optJSONObject(index) ?: continue
                        add(
                            SettingsDevice(
                                deviceId = device.optString("device_id"),
                                deviceName = device.optString("device_name"),
                                secondaryText = device.optString("secondary_text")
                                    .ifEmpty { device.optString("detail") },
                                isOnline = device.optBoolean("is_online"),
                                isTrusted = device.optBoolean("is_trusted"),
                            ),
                        )
                    }
                }
            }.orEmpty(),
            status = optString("status").takeIf { it.isNotEmpty() },
        )
    }

    private fun deviceNameHint(): String {
        val manufacturer = Build.MANUFACTURER.orEmpty().trim()
        val model = Build.MODEL.orEmpty().trim()
        return listOf(manufacturer, model)
            .filter { it.isNotEmpty() }
            .joinToString(" ")
            .ifEmpty { "Android" }
    }
}
