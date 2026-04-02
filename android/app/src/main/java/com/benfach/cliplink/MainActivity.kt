package com.benfach.cliplink

import android.Manifest
import android.content.BroadcastReceiver
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.text.Editable
import android.text.TextWatcher
import android.view.Gravity
import android.view.View
import android.widget.AdapterView
import android.widget.ArrayAdapter
import android.widget.Button
import android.widget.CheckBox
import android.widget.EditText
import android.widget.ImageButton
import android.widget.LinearLayout
import android.widget.PopupMenu
import android.widget.ScrollView
import android.widget.Spinner
import android.widget.Switch
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import org.json.JSONObject

class MainActivity : AppCompatActivity() {
    private lateinit var historyTabButton: Button
    private lateinit var preferencesTabButton: Button
    private lateinit var historyContainer: LinearLayout
    private lateinit var preferencesContainer: ScrollView
    private lateinit var historyScopeSpinner: Spinner
    private lateinit var historyScopeAdapter: ArrayAdapter<String>
    private lateinit var searchInput: EditText
    private lateinit var historyList: RecyclerView
    private lateinit var clearButton: ImageButton
    private lateinit var savePreferencesButton: Button
    private lateinit var deviceNameInput: EditText
    private lateinit var historyLimitInput: EditText
    private lateinit var shareLocalSwitch: Switch
    private lateinit var preferRemoteSwitch: Switch
    private lateinit var discoverySwitch: Switch
    private lateinit var devicesList: RecyclerView
    private lateinit var devicesEmptyText: TextView

    private lateinit var clipboardManager: ClipboardManager
    private val adapter = HistoryAdapter(
        onItemClicked = { item -> onHistoryItemClicked(item) },
        onItemLongPressed = { anchor, item -> showHistoryItemActions(anchor, item) },
    )
    private val deviceAdapter = SettingsDeviceAdapter(
        onSelectionChanged = { device, isSelected ->
            onDeviceSelectionChanged(device.deviceId, isSelected)
        },
        onItemLongPressed = { anchor, device ->
            showDeviceActionsMenu(anchor, device)
        },
    )
    private lateinit var transferPanelController: TransferPanelController

    private var currentHistory: List<RustBridge.HistoryItem> = emptyList()
    private var currentHistoryScopes: List<RustBridge.HistoryScopeOption> = emptyList()
    private var currentSettings: RustBridge.SettingsSnapshot? = null
    private var currentDevices: List<RustBridge.SettingsDevice> = emptyList()
    private val selectedDeviceIds = linkedSetOf<String>()
    private var currentHistoryScope: String = HISTORY_SCOPE_ALL
    private var suppressHistoryScopeCallbacks = false
    private var preferencesDirty = false
    private var suppressPreferenceCallbacks = false
    private var stateReceiverRegistered = false
    private var clipboardListenerRegistered = false
    private var activeTrustPromptId: String? = null
    private var bridgeStarted = false
    private val uiHandler = Handler(Looper.getMainLooper())
    private val refreshSnapshotRunnable = Runnable { refreshSnapshot() }
    private val clipboardRetryRunnable = Runnable {
        if (bridgeStarted && hasWindowFocus()) {
            submitCurrentClipboard(scheduleRetry = false)
        }
    }
    private var actionToast: Toast? = null

    private val clipboardChangedListener = ClipboardManager.OnPrimaryClipChangedListener {
        if (!bridgeStarted || !hasWindowFocus()) {
            return@OnPrimaryClipChangedListener
        }
        submitCurrentClipboard()
    }

    private val stateChangedReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            requestSnapshotRefresh(SERVICE_REFRESH_DELAY_MS)
            val runtimeError = intent?.getStringExtra(ClipboardMonitorService.EXTRA_RUNTIME_ERROR)
            if (!runtimeError.isNullOrBlank()) {
                updateStatusText(runtimeError)
            }
        }
    }

    companion object {
        private const val HISTORY_ACTION_PIN = 1
        private const val HISTORY_ACTION_DELETE = 2
        private const val HISTORY_ACTION_BATCH = 3
        private const val DEVICE_ACTION_TRUST = 11
        private const val DEVICE_ACTION_REVOKE = 12
        private const val DEVICE_ACTION_PROPERTIES = 13
        private const val REQUEST_RUNTIME_PERMISSIONS = 5001
        private const val SEARCH_REFRESH_DELAY_MS = 140L
        private const val SERVICE_REFRESH_DELAY_MS = 48L
        private const val FOREGROUND_CLIPBOARD_RETRY_MS = 96L
        private const val HISTORY_SCOPE_ALL = "all"
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        clipboardManager = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        bindViews()
        transferPanelController = TransferPanelController(this, ::showActionFeedback)
        configureHistoryPane()
        configurePreferencesPane()
        configureTabs()
        ensurePermissionsAndStart()
    }

    override fun onStart() {
        super.onStart()
        registerStateChangedReceiver()
        registerClipboardListener()
    }

    override fun onResume() {
        super.onResume()
        if (bridgeStarted) {
            submitCurrentClipboard()
            requestSnapshotRefresh()
        }
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (!hasFocus || !bridgeStarted) {
            return
        }
        submitCurrentClipboard(scheduleRetry = true)
        requestSnapshotRefresh()
    }

    override fun onStop() {
        super.onStop()
        if (stateReceiverRegistered) {
            unregisterReceiver(stateChangedReceiver)
            stateReceiverRegistered = false
        }
        if (clipboardListenerRegistered) {
            clipboardManager.removePrimaryClipChangedListener(clipboardChangedListener)
            clipboardListenerRegistered = false
        }
        uiHandler.removeCallbacks(refreshSnapshotRunnable)
        uiHandler.removeCallbacks(clipboardRetryRunnable)
    }

    private fun bindViews() {
        historyTabButton = findViewById(R.id.button_history_tab)
        preferencesTabButton = findViewById(R.id.button_preferences_tab)
        historyContainer = findViewById(R.id.history_container)
        preferencesContainer = findViewById(R.id.preferences_container)
        historyScopeSpinner = findViewById(R.id.spinner_history_scope)
        searchInput = findViewById(R.id.input_search)
        historyList = findViewById(R.id.history_list)
        clearButton = findViewById(R.id.button_clear)
        savePreferencesButton = findViewById(R.id.button_save_preferences)
        deviceNameInput = findViewById(R.id.input_device_name)
        historyLimitInput = findViewById(R.id.input_history_limit)
        shareLocalSwitch = findViewById(R.id.switch_share_local_history)
        preferRemoteSwitch = findViewById(R.id.switch_prefer_remote_latest)
        discoverySwitch = findViewById(R.id.switch_discovery_enabled)
        devicesList = findViewById(R.id.list_devices)
        devicesEmptyText = findViewById(R.id.text_devices_empty)
        updateStatusText(null)
    }

    private fun configureHistoryPane() {
        historyList.layoutManager = LinearLayoutManager(this)
        historyList.itemAnimator = null
        historyList.adapter = adapter

        historyScopeAdapter = ArrayAdapter(
            this,
            android.R.layout.simple_spinner_item,
            mutableListOf<String>(),
        )
        historyScopeAdapter.setDropDownViewResource(android.R.layout.simple_spinner_dropdown_item)
        historyScopeSpinner.adapter = historyScopeAdapter
        historyScopeSpinner.onItemSelectedListener = object : AdapterView.OnItemSelectedListener {
            override fun onItemSelected(
                parent: AdapterView<*>?,
                view: View?,
                position: Int,
                id: Long,
            ) {
                if (suppressHistoryScopeCallbacks) {
                    return
                }
                val nextScope = currentHistoryScopes
                    .getOrNull(position)
                    ?.key
                    ?: HISTORY_SCOPE_ALL
                val changed = nextScope != currentHistoryScope
                currentHistoryScope = nextScope
                if (bridgeStarted && changed) {
                    requestSnapshotRefresh(SEARCH_REFRESH_DELAY_MS)
                }
            }

            override fun onNothingSelected(parent: AdapterView<*>?) = Unit
        }

        searchInput.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) = Unit
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) = Unit
            override fun afterTextChanged(s: Editable?) {
                if (bridgeStarted) {
                    requestSnapshotRefresh(SEARCH_REFRESH_DELAY_MS)
                }
            }
        })

        clearButton.setOnClickListener {
            showClearHistoryConfirm()
        }
    }

    private fun showClearHistoryConfirm() {
        val density = resources.displayMetrics.density
        val horizontalPadding = (24f * density).toInt()
        val topPadding = (8f * density).toInt()
        val checkboxTopMargin = (12f * density).toInt()
        val contentView = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(horizontalPadding, topPadding, horizontalPadding, 0)
        }
        val messageView = TextView(this).apply {
            text = getString(R.string.clear_history_confirm_message)
        }
        val includePinnedCheckbox = CheckBox(this).apply {
            text = getString(R.string.clear_history_include_pinned)
            isChecked = false
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT,
                LinearLayout.LayoutParams.WRAP_CONTENT,
            ).apply {
                gravity = Gravity.CENTER_HORIZONTAL
                topMargin = checkboxTopMargin
            }
        }
        contentView.addView(messageView)
        contentView.addView(includePinnedCheckbox)

        AlertDialog.Builder(this)
            .setTitle(getString(R.string.clear_history_confirm_title))
            .setView(contentView)
            .setPositiveButton(getString(R.string.clear_history_confirm_action)) { _, _ ->
                val error = RustBridge.nativeClearHistory(includePinnedCheckbox.isChecked)
                if (error != null) {
                    showActionFeedback(error)
                    return@setPositiveButton
                }
                refreshSnapshot(showFeedback = true)
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }

    private fun configurePreferencesPane() {
        val watcher = object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) = Unit
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) = Unit
            override fun afterTextChanged(s: Editable?) {
                if (suppressPreferenceCallbacks) {
                    return
                }
                preferencesDirty = true
            }
        }

        deviceNameInput.addTextChangedListener(watcher)
        historyLimitInput.addTextChangedListener(watcher)

        shareLocalSwitch.setOnCheckedChangeListener { _, _ ->
            if (suppressPreferenceCallbacks) {
                return@setOnCheckedChangeListener
            }
            preferencesDirty = true
            savePreferences()
        }
        preferRemoteSwitch.setOnCheckedChangeListener { _, _ ->
            if (suppressPreferenceCallbacks) {
                return@setOnCheckedChangeListener
            }
            preferencesDirty = true
        }
        discoverySwitch.setOnCheckedChangeListener { _, _ ->
            if (suppressPreferenceCallbacks) {
                return@setOnCheckedChangeListener
            }
            preferencesDirty = true
        }
        devicesList.layoutManager = LinearLayoutManager(this)
        devicesList.itemAnimator = null
        devicesList.adapter = deviceAdapter

        savePreferencesButton.setOnClickListener {
            savePreferences()
        }
    }

    private fun configureTabs() {
        historyTabButton.setOnClickListener { showHistoryTab(true) }
        preferencesTabButton.setOnClickListener { showHistoryTab(false) }
        showHistoryTab(true)
    }

    private fun showHistoryTab(showHistory: Boolean) {
        historyContainer.visibility = if (showHistory) View.VISIBLE else View.GONE
        preferencesContainer.visibility = if (showHistory) View.GONE else View.VISIBLE
        historyTabButton.isEnabled = !showHistory
        preferencesTabButton.isEnabled = showHistory
        historyTabButton.setBackgroundResource(
            if (showHistory) R.drawable.bg_tab_selected else R.drawable.bg_tab_idle,
        )
        preferencesTabButton.setBackgroundResource(
            if (showHistory) R.drawable.bg_tab_idle else R.drawable.bg_tab_selected,
        )
        historyTabButton.setTextColor(
            ContextCompat.getColor(
                this,
                if (showHistory) R.color.color_text_primary else R.color.color_text_secondary,
            ),
        )
        preferencesTabButton.setTextColor(
            ContextCompat.getColor(
                this,
                if (showHistory) R.color.color_text_secondary else R.color.color_text_primary,
            ),
        )
    }

    private fun submitCurrentClipboard(scheduleRetry: Boolean = false) {
        val submission = ClipboardInterop.submitCurrentClipboard(this, clipboardManager)
        if (submission.error != null) {
            updateStatusText(submission.error)
        }
        if (submission.submitted) {
            uiHandler.removeCallbacks(clipboardRetryRunnable)
            requestSnapshotRefresh()
            return
        }
        if (scheduleRetry && submission.requiresForegroundAccess && hasWindowFocus()) {
            uiHandler.removeCallbacks(clipboardRetryRunnable)
            uiHandler.postDelayed(clipboardRetryRunnable, FOREGROUND_CLIPBOARD_RETRY_MS)
        }
    }

    private fun registerClipboardListener() {
        if (clipboardListenerRegistered) {
            return
        }
        clipboardManager.addPrimaryClipChangedListener(clipboardChangedListener)
        clipboardListenerRegistered = true
    }

    private fun ensurePermissionsAndStart() {
        val missingPermissions = missingRuntimePermissions()
        if (missingPermissions.isEmpty()) {
            startBridgeAndSync()
            return
        }

        AlertDialog.Builder(this)
            .setTitle(getString(R.string.permissions_request_title))
            .setMessage(buildPermissionsMessage(missingPermissions))
            .setCancelable(false)
            .setPositiveButton(getString(R.string.permissions_request_confirm)) { _, _ ->
                ActivityCompat.requestPermissions(
                    this,
                    missingPermissions.toTypedArray(),
                    REQUEST_RUNTIME_PERMISSIONS,
                )
            }
            .setNegativeButton(getString(R.string.permissions_request_skip)) { _, _ ->
                updateStatusText(getString(R.string.permissions_limited_mode))
                startBridgeAndSync()
            }
            .show()
    }

    private fun startBridgeAndSync() {
        if (bridgeStarted) {
            return
        }

        val bootstrapError = RustBridge.bootstrap(this)
        if (bootstrapError != null) {
            updateStatusText(bootstrapError)
            Toast.makeText(this, bootstrapError, Toast.LENGTH_LONG).show()
            return
        }

        bridgeStarted = true
        ClipboardMonitorService.start(this)
        requestSnapshotRefresh()
    }

    private fun missingRuntimePermissions(): List<String> {
        val requiredPermissions = mutableListOf<String>()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            requiredPermissions += Manifest.permission.POST_NOTIFICATIONS
            requiredPermissions += Manifest.permission.READ_MEDIA_IMAGES
            requiredPermissions += Manifest.permission.READ_MEDIA_VIDEO
            requiredPermissions += Manifest.permission.READ_MEDIA_AUDIO
        } else {
            requiredPermissions += Manifest.permission.READ_EXTERNAL_STORAGE
        }

        return requiredPermissions.filter { permission ->
            ContextCompat.checkSelfPermission(this, permission) != PackageManager.PERMISSION_GRANTED
        }
    }

    private fun buildPermissionsMessage(missingPermissions: List<String>): String {
        val lines = missingPermissions.joinToString("\n") { permission ->
            "• ${permissionDisplayName(permission)}"
        }
        return getString(R.string.permissions_request_message, lines)
    }

    private fun permissionDisplayName(permission: String): String {
        return when (permission) {
            Manifest.permission.POST_NOTIFICATIONS -> getString(R.string.permission_notifications)
            Manifest.permission.READ_MEDIA_IMAGES -> getString(R.string.permission_media_images)
            Manifest.permission.READ_MEDIA_VIDEO -> getString(R.string.permission_media_videos)
            Manifest.permission.READ_MEDIA_AUDIO -> getString(R.string.permission_media_audio)
            Manifest.permission.READ_EXTERNAL_STORAGE -> getString(R.string.permission_storage)
            else -> permission
        }
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode != REQUEST_RUNTIME_PERMISSIONS) {
            return
        }

        val deniedPermissions = permissions.indices
            .filter { index ->
                index >= grantResults.size || grantResults[index] != PackageManager.PERMISSION_GRANTED
            }
            .map { index -> permissions[index] }

        if (deniedPermissions.isEmpty()) {
            startBridgeAndSync()
            return
        }

        updateStatusText(getString(R.string.permissions_limited_mode))
        showPermissionsDeniedDialog(deniedPermissions)
        startBridgeAndSync()
    }

    private fun showPermissionsDeniedDialog(deniedPermissions: List<String>) {
        val deniedLines = deniedPermissions.joinToString("\n") { permission ->
            "• ${permissionDisplayName(permission)}"
        }
        AlertDialog.Builder(this)
            .setTitle(getString(R.string.permissions_denied_title))
            .setMessage(getString(R.string.permissions_denied_message, deniedLines))
            .setPositiveButton(getString(R.string.permissions_open_settings)) { _, _ ->
                openAppPermissionSettings()
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }

    private fun openAppPermissionSettings() {
        val intent = Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS).apply {
            data = Uri.fromParts("package", packageName, null)
        }
        startActivity(intent)
    }

    private fun activateHistoryItem(itemId: String) {
        val result = RustBridge.activateHistoryItem(itemId)
        if (result.error != null) {
            showActionFeedback(result.error)
            return
        }
        if (result.transferPending) {
            showActionFeedback(getString(R.string.history_file_transfer_in_progress))
        }
        transferPanelController.render(result.transferProgress)
        applyPendingClipboard(result.pendingClipboard)
        refreshSnapshot()
    }

    private fun onHistoryItemClicked(item: RustBridge.HistoryItem) {
        val isFileItem = item.kind.equals("files", ignoreCase = true)
        if (isFileItem && item.isRemote) {
            showRemoteFileSaveConfirm(item.id)
            return
        }
        activateHistoryItem(item.id)
    }

    private fun showRemoteFileSaveConfirm(itemId: String) {
        AlertDialog.Builder(this)
            .setTitle(getString(R.string.history_remote_file_save_title))
            .setMessage(getString(R.string.history_remote_file_save_message))
            .setPositiveButton(getString(R.string.history_remote_file_save_confirm)) { _, _ ->
                activateHistoryItem(itemId)
            }
            .setNegativeButton(getString(R.string.history_remote_file_save_cancel), null)
            .show()
    }

    private fun showHistoryItemActions(anchor: View, item: RustBridge.HistoryItem) {
        if (item.isRemote) {
            Toast.makeText(this, getString(R.string.history_item_readonly), Toast.LENGTH_SHORT).show()
            return
        }

        val popupMenu = PopupMenu(this, anchor)
        popupMenu.menu.add(
            0,
            HISTORY_ACTION_PIN,
            0,
            getString(if (item.isPinned) R.string.unpin_history_item else R.string.pin_history_item),
        )
        popupMenu.menu.add(0, HISTORY_ACTION_DELETE, 1, getString(R.string.delete_history_item))
        if (editableLocalHistoryItems().size > 1) {
            popupMenu.menu.add(0, HISTORY_ACTION_BATCH, 2, getString(R.string.batch_manage_history))
        }
        popupMenu.setOnMenuItemClickListener { menuItem ->
            when (menuItem.itemId) {
                HISTORY_ACTION_PIN -> {
                    toggleHistoryItemPin(item.id)
                    true
                }

                HISTORY_ACTION_DELETE -> {
                    deleteHistoryItem(item.id)
                    true
                }

                HISTORY_ACTION_BATCH -> {
                    showHistoryBatchActionsDialog(item.id)
                    true
                }

                else -> false
            }
        }
        popupMenu.show()
    }

    private fun deleteHistoryItem(itemId: String) {
        val error = RustBridge.deleteHistoryItem(itemId)
        if (error != null) {
            showActionFeedback(error)
            return
        }
        refreshSnapshot(showFeedback = true)
    }

    private fun toggleHistoryItemPin(itemId: String) {
        val error = RustBridge.toggleHistoryItemPin(itemId)
        if (error != null) {
            showActionFeedback(error)
            return
        }
        refreshSnapshot(showFeedback = true)
    }

    private fun refreshSnapshot(showFeedback: Boolean = false): RustBridge.Snapshot? {
        val snapshot = RustBridge.refreshSnapshot(
            searchInput.text?.toString().orEmpty(),
            currentHistoryScope,
        )
        if (snapshot.error != null) {
            if (showFeedback) {
                showActionFeedback(snapshot.error)
            } else {
                updateStatusText(snapshot.error)
            }
            return null
        }

        renderHistoryScopes(snapshot.historyScopes, snapshot.selectedHistoryScope)
        currentHistory = snapshot.history
        adapter.submitList(snapshot.history)
        transferPanelController.render(snapshot.transferProgress)
        applyPendingClipboard(snapshot.pendingClipboard)
        snapshot.settings?.let { settings ->
            currentSettings = settings
            updateStatusText(settings.status)
            renderDevices(settings.devices)
            if (!preferencesDirty) {
                populateSettingsForm(settings)
            }
        }
        showPendingTrustRequest(snapshot.pendingTrustRequest)
        if (showFeedback) {
            showActionFeedback(snapshot.settings?.status)
        }
        return snapshot
    }

    private fun renderHistoryScopes(
        scopes: List<RustBridge.HistoryScopeOption>,
        selectedScope: String,
    ) {
        val normalizedScopes = if (scopes.isEmpty()) {
            listOf(RustBridge.HistoryScopeOption(key = HISTORY_SCOPE_ALL, label = getString(R.string.history_scope_all)))
        } else {
            scopes
        }

        currentHistoryScopes = normalizedScopes
        currentHistoryScope = normalizedScopes
            .firstOrNull { it.key == selectedScope }
            ?.key
            ?: HISTORY_SCOPE_ALL

        val labels = normalizedScopes.map { it.label }
        val needsDatasetUpdate =
            historyScopeAdapter.count != labels.size || labels.indices.any { index ->
                historyScopeAdapter.getItem(index) != labels[index]
            }
        if (needsDatasetUpdate) {
            historyScopeAdapter.clear()
            historyScopeAdapter.addAll(labels)
            historyScopeAdapter.notifyDataSetChanged()
        }

        val selectedIndex = normalizedScopes.indexOfFirst { it.key == currentHistoryScope }
            .takeIf { it >= 0 }
            ?: 0
        if (historyScopeSpinner.selectedItemPosition != selectedIndex) {
            suppressHistoryScopeCallbacks = true
            historyScopeSpinner.setSelection(selectedIndex, false)
            suppressHistoryScopeCallbacks = false
        }
    }

    private fun requestSnapshotRefresh(delayMs: Long = 0L) {
        uiHandler.removeCallbacks(refreshSnapshotRunnable)
        uiHandler.postDelayed(refreshSnapshotRunnable, delayMs)
    }

    @Suppress("UNUSED_PARAMETER")
    private fun updateStatusText(messageIgnored: String?) = Unit

    private fun showActionFeedback(message: String?) {
        val text = message?.trim().orEmpty()
        if (text.isEmpty()) {
            return
        }
        actionToast?.cancel()
        actionToast = Toast.makeText(this, text, Toast.LENGTH_SHORT)
        actionToast?.show()
    }

    private fun populateSettingsForm(settings: RustBridge.SettingsSnapshot) {
        suppressPreferenceCallbacks = true
        deviceNameInput.setText(settings.deviceName)
        historyLimitInput.setText(settings.historyLimit.toString())
        shareLocalSwitch.isChecked = settings.shareLocalHistory
        preferRemoteSwitch.isChecked = settings.preferRemoteLatestOnPaste
        discoverySwitch.isChecked = settings.discoveryEnabled
        suppressPreferenceCallbacks = false
        preferencesDirty = false
    }

    private fun renderDevices(devices: List<RustBridge.SettingsDevice>) {
        currentDevices = devices
        selectedDeviceIds.retainAll(devices.map { it.deviceId }.toSet())
        deviceAdapter.submitDevices(devices, selectedDeviceIds)
        val isEmpty = devices.isEmpty()
        devicesEmptyText.visibility = if (isEmpty) View.VISIBLE else View.GONE
        devicesList.visibility = if (isEmpty) View.GONE else View.VISIBLE
    }

    private fun showDeviceProperties(device: RustBridge.SettingsDevice) {
        val statusUi = device.toDeviceStatusUi(this)
        val message = buildString {
            append("设备名称：")
            append(device.deviceName)
            append('\n')
            append("设备 ID：")
            append(device.deviceId)
            append('\n')
            append("状态：")
            append(statusUi.label)
            if (device.secondaryText.isNotBlank()) {
                append('\n')
                append("地址：")
                append(device.secondaryText)
            }
            if (device.statusTooltip.isNotBlank()) {
                append("\n\n")
                append(device.statusTooltip)
            }
        }

        AlertDialog.Builder(this)
            .setTitle(getString(R.string.device_properties))
            .setMessage(message)
            .setPositiveButton(android.R.string.ok, null)
            .show()
    }

    private fun showDeviceActionsMenu(anchor: View, device: RustBridge.SettingsDevice) {
        val selectedDevices = resolveDeviceActionSelection(device.deviceId)
        if (selectedDevices.isEmpty()) {
            Toast.makeText(this, getString(R.string.choose_device_first), Toast.LENGTH_SHORT).show()
            return
        }

        val popupMenu = PopupMenu(this, anchor)
        val trustedDevices = selectedDevices.filter { it.isTrusted }
        val untrustedDevices = selectedDevices.filter { !it.isTrusted }

        if (selectedDevices.size == 1) {
            if (device.isTrusted) {
                popupMenu.menu.add(0, DEVICE_ACTION_REVOKE, 0, getString(R.string.revoke_device))
            } else {
                popupMenu.menu.add(0, DEVICE_ACTION_TRUST, 0, getString(R.string.trust_device))
            }
            popupMenu.menu.add(0, DEVICE_ACTION_PROPERTIES, 1, getString(R.string.device_properties))
        } else {
            if (untrustedDevices.isNotEmpty()) {
                popupMenu.menu.add(
                    0,
                    DEVICE_ACTION_TRUST,
                    0,
                    getString(R.string.trust_selected_devices, untrustedDevices.size),
                )
            }
            if (trustedDevices.isNotEmpty()) {
                popupMenu.menu.add(
                    0,
                    DEVICE_ACTION_REVOKE,
                    1,
                    getString(R.string.revoke_selected_devices, trustedDevices.size),
                )
            }
        }

        popupMenu.setOnMenuItemClickListener { menuItem ->
            when (menuItem.itemId) {
                DEVICE_ACTION_TRUST -> {
                    trustDevices(untrustedDevices.map { it.deviceId })
                    true
                }
                DEVICE_ACTION_REVOKE -> {
                    revokeTrustedDevices(trustedDevices)
                    true
                }
                DEVICE_ACTION_PROPERTIES -> {
                    selectedDevices.singleOrNull()?.let(::showDeviceProperties)
                    true
                }
                else -> false
            }
        }
        popupMenu.show()
    }

    private fun onDeviceSelectionChanged(deviceId: String, isSelected: Boolean) {
        if (isSelected) {
            selectedDeviceIds += deviceId
        } else {
            selectedDeviceIds -= deviceId
        }
        deviceAdapter.submitDevices(currentDevices, selectedDeviceIds)
    }

    private fun resolveDeviceActionSelection(deviceId: String): List<RustBridge.SettingsDevice> {
        if (deviceId !in selectedDeviceIds) {
            selectedDeviceIds.clear()
            selectedDeviceIds += deviceId
            deviceAdapter.submitDevices(currentDevices, selectedDeviceIds)
        }
        return currentDevices.filter { it.deviceId in selectedDeviceIds }
    }

    private fun trustDevices(deviceIds: List<String>) {
        if (deviceIds.isEmpty()) {
            Toast.makeText(this, getString(R.string.choose_device_first), Toast.LENGTH_SHORT).show()
            return
        }

        val error = RustBridge.trustDevices(deviceIds)
        if (error != null) {
            showActionFeedback(error)
            return
        }
        selectedDeviceIds.clear()
        selectedDeviceIds.addAll(deviceIds)
        refreshSnapshot(showFeedback = true)
    }

    private fun revokeTrustedDevices(devices: List<RustBridge.SettingsDevice>) {
        if (devices.isEmpty()) {
            Toast.makeText(this, getString(R.string.choose_device_first), Toast.LENGTH_SHORT).show()
            return
        }

        val message = if (devices.size == 1) {
            getString(R.string.revoke_device_confirm_message, devices[0].deviceName)
        } else {
            getString(R.string.revoke_devices_confirm_message, devices.size)
        }
        AlertDialog.Builder(this)
            .setTitle(getString(R.string.revoke_device_confirm_title))
            .setMessage(message)
            .setPositiveButton(getString(R.string.revoke_device_confirm_action)) { _, _ ->
                val error = RustBridge.revokeTrustedDevices(devices.map { it.deviceId })
                if (error != null) {
                    showActionFeedback(error)
                    return@setPositiveButton
                }
                selectedDeviceIds.removeAll(devices.map { it.deviceId }.toSet())
                refreshSnapshot(showFeedback = true)
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }

    private fun showPendingTrustRequest(request: RustBridge.PendingTrustRequest?) {
        if (request == null || activeTrustPromptId == request.requestId) {
            return
        }

        activeTrustPromptId = request.requestId
        AlertDialog.Builder(this)
            .setTitle(getString(R.string.trust_request_title))
            .setMessage(buildTrustRequestMessage(request))
            .setCancelable(false)
            .setPositiveButton(getString(R.string.trust_request_allow)) { _, _ ->
                activeTrustPromptId = null
                val error = RustBridge.respondTrustRequest(request.requestId, true)
                if (error != null) {
                    showActionFeedback(error)
                }
                refreshSnapshot(showFeedback = true)
            }
            .setNegativeButton(getString(R.string.trust_request_deny)) { _, _ ->
                activeTrustPromptId = null
                val error = RustBridge.respondTrustRequest(request.requestId, false)
                if (error != null) {
                    showActionFeedback(error)
                }
                refreshSnapshot(showFeedback = true)
            }
            .show()
    }

    private fun buildTrustRequestMessage(request: RustBridge.PendingTrustRequest): String {
        return buildString {
            append(
                getString(
                    R.string.trust_request_message,
                    request.deviceName,
                ),
            )
            if (request.secondaryText.isNotBlank()) {
                append("\n\n")
                append(getString(R.string.trust_request_address, request.secondaryText))
            }
            if (request.fingerprint.isNotBlank()) {
                append("\n")
                append(getString(R.string.trust_request_fingerprint, request.fingerprint))
            }
        }
    }

    private fun savePreferences() {
        val snapshot = currentSettings ?: return
        val payload = JSONObject()
            .put("device_name", deviceNameInput.text?.toString().orEmpty())
            .put("history_limit", historyLimitInput.text?.toString().orEmpty().toIntOrNull() ?: snapshot.historyLimit)
            .put("hotkey", snapshot.hotkey)
            .put("share_local_history", shareLocalSwitch.isChecked)
            .put("prefer_remote_latest_on_paste", preferRemoteSwitch.isChecked)
            .put("discovery_enabled", discoverySwitch.isChecked)

        val error = RustBridge.nativeSaveSettings(payload.toString())
        if (error != null) {
            showActionFeedback(error)
            return
        }

        preferencesDirty = false
        refreshSnapshot(showFeedback = true)
    }

    private fun applyPendingClipboard(payload: RustBridge.PendingClipboard?) {
        val applied = ClipboardInterop.applyPendingClipboard(this, clipboardManager, payload)
        if (applied) {
            transferPanelController.onPendingClipboardApplied()
        }
    }

    private fun editableLocalHistoryItems(): List<RustBridge.HistoryItem> {
        return currentHistory.filter { !it.isRemote }
    }

    private fun showHistoryBatchActionsDialog(initialItemId: String?) {
        val items = editableLocalHistoryItems()
        if (items.isEmpty()) {
            showActionFeedback(getString(R.string.history_item_readonly))
            return
        }

        val selectedIds = mutableSetOf<String>()
        initialItemId?.let { initialId ->
            if (items.any { it.id == initialId }) {
                selectedIds += initialId
            }
        }
        val labels = items.map { item ->
            buildString {
                append(item.summaryText.ifBlank { getString(R.string.empty_history_summary) })
                if (item.isPinned) {
                    append(" · ")
                    append(getString(R.string.history_item_pinned_label))
                }
            }
        }.toTypedArray()
        val checkedItems = BooleanArray(items.size) { index ->
            items[index].id in selectedIds
        }

        AlertDialog.Builder(this)
            .setTitle(getString(R.string.batch_manage_history))
            .setMultiChoiceItems(labels, checkedItems) { _, which, isChecked ->
                val itemId = items.getOrNull(which)?.id ?: return@setMultiChoiceItems
                if (isChecked) {
                    selectedIds += itemId
                } else {
                    selectedIds -= itemId
                }
            }
            .setPositiveButton(getString(R.string.delete_history_item)) { _, _ ->
                deleteHistoryItems(selectedIds.toList())
            }
            .setNeutralButton(getString(R.string.batch_toggle_pin_history)) { _, _ ->
                applyBatchHistoryPin(selectedIds.toList())
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }

    private fun deleteHistoryItems(itemIds: List<String>) {
        if (itemIds.isEmpty()) {
            showActionFeedback(getString(R.string.batch_select_history_first))
            return
        }

        val error = RustBridge.deleteHistoryItems(itemIds)
        if (error != null) {
            showActionFeedback(error)
            return
        }
        refreshSnapshot(showFeedback = true)
    }

    private fun applyBatchHistoryPin(itemIds: List<String>) {
        if (itemIds.isEmpty()) {
            showActionFeedback(getString(R.string.batch_select_history_first))
            return
        }

        val selectedItems = editableLocalHistoryItems().filter { it.id in itemIds }
        if (selectedItems.isEmpty()) {
            showActionFeedback(getString(R.string.history_item_readonly))
            return
        }

        val nextPinned = !selectedItems.all { it.isPinned }
        val error = RustBridge.setHistoryItemsPinned(
            selectedItems.map { it.id },
            nextPinned,
        )
        if (error != null) {
            showActionFeedback(error)
            return
        }
        refreshSnapshot(showFeedback = true)
    }

    private fun registerStateChangedReceiver() {
        if (stateReceiverRegistered) {
            return
        }

        val filter = IntentFilter(ClipboardMonitorService.ACTION_STATE_CHANGED)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            ContextCompat.registerReceiver(
                this,
                stateChangedReceiver,
                filter,
                ContextCompat.RECEIVER_NOT_EXPORTED,
            )
        } else {
            @Suppress("DEPRECATION")
            registerReceiver(stateChangedReceiver, filter)
        }
        stateReceiverRegistered = true
    }

}
