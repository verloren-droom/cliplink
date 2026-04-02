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
        val kind: String,
        val sourceBadge: String,
        val sourceTooltip: String?,
        val summaryText: String,
        val detailTooltip: String?,
        val isRemote: Boolean,
        val isPinned: Boolean,
    )

    data class SettingsDevice(
        val deviceId: String,
        val deviceName: String,
        val secondaryText: String,
        val statusTooltip: String,
        val statusKind: String,
        val isOnline: Boolean,
        val isTrusted: Boolean,
    )

    data class HistoryScopeOption(
        val key: String,
        val label: String,
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

    data class PendingTrustRequest(
        val requestId: String,
        val deviceId: String,
        val deviceName: String,
        val secondaryText: String,
        val fingerprint: String,
    )

    data class Snapshot(
        val history: List<HistoryItem>,
        val historyScopes: List<HistoryScopeOption>,
        val selectedHistoryScope: String,
        val settings: SettingsSnapshot?,
        val transferProgress: TransferProgress?,
        val pendingClipboard: PendingClipboard?,
        val pendingTrustRequest: PendingTrustRequest?,
        val error: String?,
    )

    data class ActionResult(
        val pendingClipboard: PendingClipboard?,
        val transferPending: Boolean,
        val transferProgress: TransferProgress?,
        val error: String?,
    )

    data class TickResult(
        val pendingClipboard: PendingClipboard?,
        val historyChanged: Boolean,
        val devicesChanged: Boolean,
        val statusChanged: Boolean,
        val transferChanged: Boolean,
        val transferProgress: TransferProgress?,
        val error: String?,
    )

    data class TransferProgress(
        val itemId: String,
        val label: String,
        val detail: String,
        val fraction: Double,
        val sourceDeviceName: String,
        val summary: String,
        val bytesDone: Long,
        val bytesTotal: Long,
    )

    data class PendingClipboard(
        val kind: String,
        val text: String?,
        val paths: List<String>,
        val itemId: String?,
        val summary: String?,
        val sourceDeviceName: String?,
    )

    external fun nativeBootstrap(filesDir: String, deviceNameHint: String, localDataKey: ByteArray): String?
    external fun nativeSubmitClipboardText(text: String): String?
    external fun nativeSubmitClipboardFiles(pathsJson: String): String?
    external fun nativeRefreshSnapshot(query: String, scope: String): String
    external fun nativeTick(): String
    external fun nativeActivateHistoryItem(itemId: String): String
    external fun nativeDeleteHistoryItem(itemId: String): String?
    external fun nativeDeleteHistoryItems(itemIdsJson: String): String?
    external fun nativeToggleHistoryItemPin(itemId: String): String?
    external fun nativeSetHistoryItemsPinned(itemIdsJson: String, pinned: Boolean): String?
    external fun nativeClearHistory(includePinned: Boolean): String?
    external fun nativeSaveSettings(settingsJson: String): String?
    external fun nativeTrustDevice(deviceId: String): String?
    external fun nativeTrustDevices(deviceIdsJson: String): String?
    external fun nativeRespondTrustRequest(requestId: String, allow: Boolean): String?
    external fun nativeRevokeTrustedDevice(deviceId: String): String?
    external fun nativeRevokeTrustedDevices(deviceIdsJson: String): String?

    fun bootstrap(context: Context): String? {
        val localDataKey = AndroidKeyStoreBridge.loadOrCreateLocalDataKey(context)
        return nativeBootstrap(context.filesDir.absolutePath, deviceNameHint(), localDataKey)
    }

    fun refreshSnapshot(query: String, scope: String): Snapshot {
        return parseSnapshot(nativeRefreshSnapshot(query, scope))
    }

    fun submitClipboardFiles(paths: List<String>): String? {
        return nativeSubmitClipboardFiles(JSONArray(paths).toString())
    }

    fun tick(): TickResult {
        val payload = JSONObject(nativeTick())
        return TickResult(
            pendingClipboard = payload.optJSONObject("pending_clipboard")?.toPendingClipboard(),
            historyChanged = payload.optBoolean("history_changed"),
            devicesChanged = payload.optBoolean("devices_changed"),
            statusChanged = payload.optBoolean("status_changed"),
            transferChanged = payload.optBoolean("transfer_changed"),
            transferProgress = payload.optJSONObject("transfer_progress")?.toTransferProgress(),
            error = payload.optNullableString("error"),
        )
    }

    fun activateHistoryItem(itemId: String): ActionResult {
        val payload = JSONObject(nativeActivateHistoryItem(itemId))
        return ActionResult(
            pendingClipboard = payload.optJSONObject("pending_clipboard")?.toPendingClipboard()
                ?: payload.optNullableString("clipboard_text")
                    ?.let {
                        PendingClipboard(
                            kind = "text",
                            text = it,
                            paths = emptyList(),
                            itemId = null,
                            summary = null,
                            sourceDeviceName = null,
                        )
                    },
            transferPending = payload.optBoolean("transfer_pending"),
            transferProgress = payload.optJSONObject("transfer_progress")?.toTransferProgress(),
            error = payload.optNullableString("error"),
        )
    }

    fun deleteHistoryItem(itemId: String): String? {
        return nativeDeleteHistoryItem(itemId)
    }

    fun deleteHistoryItems(itemIds: List<String>): String? {
        return nativeDeleteHistoryItems(JSONArray(itemIds).toString())
    }

    fun toggleHistoryItemPin(itemId: String): String? {
        return nativeToggleHistoryItemPin(itemId)
    }

    fun setHistoryItemsPinned(itemIds: List<String>, pinned: Boolean): String? {
        return nativeSetHistoryItemsPinned(JSONArray(itemIds).toString(), pinned)
    }

    fun trustDevice(deviceId: String): String? {
        return nativeTrustDevice(deviceId)
    }

    fun trustDevices(deviceIds: List<String>): String? {
        return nativeTrustDevices(JSONArray(deviceIds).toString())
    }

    fun revokeTrustedDevice(deviceId: String): String? {
        return nativeRevokeTrustedDevice(deviceId)
    }

    fun revokeTrustedDevices(deviceIds: List<String>): String? {
        return nativeRevokeTrustedDevices(JSONArray(deviceIds).toString())
    }

    private fun parseSnapshot(json: String): Snapshot {
        val payload = JSONObject(json)
        val historyItems = payload.optJSONArray("history")?.toHistoryItems().orEmpty()
        val settings = payload.optJSONObject("settings")?.toSettingsSnapshot()
        return Snapshot(
            history = historyItems,
            historyScopes = payload.optJSONArray("history_scopes")?.toHistoryScopeOptions().orEmpty(),
            selectedHistoryScope = payload.optNullableString("selected_history_scope") ?: "all",
            settings = settings,
            transferProgress = payload.optJSONObject("transfer_progress")?.toTransferProgress(),
            pendingClipboard = payload.optJSONObject("pending_clipboard")?.toPendingClipboard()
                ?: payload.optNullableString("pending_clipboard_text")
                    ?.let {
                        PendingClipboard(
                            kind = "text",
                            text = it,
                            paths = emptyList(),
                            itemId = null,
                            summary = null,
                            sourceDeviceName = null,
                        )
                    },
            pendingTrustRequest = payload.optJSONObject("pending_trust_request")?.toPendingTrustRequest(),
            error = payload.optNullableString("error"),
        )
    }

    private fun JSONArray.toHistoryScopeOptions(): List<HistoryScopeOption> {
        return buildList(length()) {
            for (index in 0 until length()) {
                val item = getJSONObject(index)
                val key = item.optString("key")
                val label = item.optString("label")
                if (key.isNotBlank() && label.isNotBlank()) {
                    add(HistoryScopeOption(key = key, label = label))
                }
            }
        }
    }

    fun respondTrustRequest(requestId: String, allow: Boolean): String? {
        return nativeRespondTrustRequest(requestId, allow)
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
            text = optNullableString("text"),
            paths = paths,
            itemId = optNullableString("item_id"),
            summary = optNullableString("summary"),
            sourceDeviceName = optNullableString("source_device_name"),
        )
    }

    private fun JSONObject.toTransferProgress(): TransferProgress {
        return TransferProgress(
            itemId = optNullableString("item_id") ?: "",
            label = optNullableString("label") ?: "",
            detail = optNullableString("detail") ?: "",
            fraction = optDouble("fraction").coerceIn(0.0, 1.0),
            sourceDeviceName = optNullableString("source_device_name") ?: "",
            summary = optNullableString("summary") ?: "",
            bytesDone = optLong("bytes_done").coerceAtLeast(0L),
            bytesTotal = optLong("bytes_total").coerceAtLeast(0L),
        )
    }

    private fun JSONArray.toHistoryItems(): List<HistoryItem> {
        return buildList(length()) {
            for (index in 0 until length()) {
                val item = getJSONObject(index)
                add(
                    HistoryItem(
                        id = item.getString("id"),
                        kind = item.optNullableString("kind") ?: "text",
                        sourceBadge = item.optString("source_badge"),
                        sourceTooltip = item.optNullableString("source_tooltip")
                            ?: item.optNullableString("source_badge_tooltip"),
                        summaryText = item.optNullableString("summary_text")
                            ?: item.optNullableString("title")
                            ?: "",
                        detailTooltip = item.optNullableString("detail_tooltip")
                            ?: item.optNullableString("content_tooltip"),
                        isRemote = item.optBoolean("is_remote"),
                        isPinned = item.optBoolean("is_pinned"),
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
                                secondaryText = device.optNullableString("secondary_text")
                                    ?: device.optNullableString("detail")
                                    ?: "",
                                statusTooltip = device.optNullableString("status_tooltip") ?: "",
                                statusKind = device.optNullableString("status_kind")
                                    ?: "untrusted",
                                isOnline = device.optBoolean("is_online"),
                                isTrusted = device.optBoolean("is_trusted"),
                            ),
                        )
                    }
                }
            }.orEmpty(),
            status = optNullableString("status"),
        )
    }

    private fun JSONObject.toPendingTrustRequest(): PendingTrustRequest {
        return PendingTrustRequest(
            requestId = optString("request_id"),
            deviceId = optString("device_id"),
            deviceName = optString("device_name"),
            secondaryText = optNullableString("secondary_text") ?: "",
            fingerprint = optNullableString("fingerprint") ?: "",
        )
    }

    private fun JSONObject.optNullableString(key: String): String? {
        if (isNull(key)) {
            return null
        }
        return optString(key).takeIf { it.isNotEmpty() && it != "null" }
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
