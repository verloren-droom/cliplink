package com.benfach.cliplink

import android.content.BroadcastReceiver
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Build
import android.os.Bundle
import android.text.Editable
import android.text.TextWatcher
import android.view.View
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.Switch
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import org.json.JSONObject

class MainActivity : AppCompatActivity() {
    private lateinit var historyTabButton: Button
    private lateinit var preferencesTabButton: Button
    private lateinit var historyContainer: LinearLayout
    private lateinit var preferencesContainer: ScrollView
    private lateinit var searchInput: EditText
    private lateinit var historyList: RecyclerView
    private lateinit var refreshButton: Button
    private lateinit var clearButton: Button
    private lateinit var savePreferencesButton: Button
    private lateinit var statusText: TextView
    private lateinit var deviceNameInput: EditText
    private lateinit var historyLimitInput: EditText
    private lateinit var shareLocalSwitch: Switch
    private lateinit var preferRemoteSwitch: Switch
    private lateinit var discoverySwitch: Switch
    private lateinit var devicesText: TextView
    private lateinit var trustDeviceButton: Button
    private lateinit var revokeDeviceButton: Button

    private lateinit var clipboardManager: ClipboardManager
    private val adapter = HistoryAdapter { item -> activateHistoryItem(item.id) }

    private var currentSettings: RustBridge.SettingsSnapshot? = null
    private var currentDevices: List<RustBridge.SettingsDevice> = emptyList()
    private var selectedDeviceId: String? = null
    private var preferencesDirty = false
    private var stateReceiverRegistered = false

    private val stateChangedReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            refreshSnapshot()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        clipboardManager = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        bindViews()
        configureHistoryPane()
        configurePreferencesPane()
        configureTabs()

        val bootstrapError = RustBridge.bootstrap(this)
        if (bootstrapError != null) {
            statusText.text = bootstrapError
            Toast.makeText(this, bootstrapError, Toast.LENGTH_LONG).show()
        } else {
            ClipboardMonitorService.start(this)
            submitCurrentClipboard()
            refreshSnapshot()
        }
    }

    override fun onStart() {
        super.onStart()
        registerStateChangedReceiver()
    }

    override fun onResume() {
        super.onResume()
        submitCurrentClipboard()
        refreshSnapshot()
    }

    override fun onStop() {
        super.onStop()
        if (stateReceiverRegistered) {
            unregisterReceiver(stateChangedReceiver)
            stateReceiverRegistered = false
        }
    }

    private fun bindViews() {
        historyTabButton = findViewById(R.id.button_history_tab)
        preferencesTabButton = findViewById(R.id.button_preferences_tab)
        historyContainer = findViewById(R.id.history_container)
        preferencesContainer = findViewById(R.id.preferences_container)
        searchInput = findViewById(R.id.input_search)
        historyList = findViewById(R.id.history_list)
        refreshButton = findViewById(R.id.button_refresh)
        clearButton = findViewById(R.id.button_clear)
        savePreferencesButton = findViewById(R.id.button_save_preferences)
        statusText = findViewById(R.id.text_status)
        deviceNameInput = findViewById(R.id.input_device_name)
        historyLimitInput = findViewById(R.id.input_history_limit)
        shareLocalSwitch = findViewById(R.id.switch_share_local_history)
        preferRemoteSwitch = findViewById(R.id.switch_prefer_remote_latest)
        discoverySwitch = findViewById(R.id.switch_discovery_enabled)
        devicesText = findViewById(R.id.text_devices)
        trustDeviceButton = findViewById(R.id.button_trust_device)
        revokeDeviceButton = findViewById(R.id.button_revoke_device)
    }

    private fun configureHistoryPane() {
        historyList.layoutManager = LinearLayoutManager(this)
        historyList.adapter = adapter

        searchInput.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) = Unit
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) = Unit
            override fun afterTextChanged(s: Editable?) {
                refreshSnapshot()
            }
        })

        refreshButton.setOnClickListener {
            submitCurrentClipboard()
            refreshSnapshot()
        }

        clearButton.setOnClickListener {
            val error = RustBridge.nativeClearHistory()
            if (error != null) {
                statusText.text = error
            }
            refreshSnapshot()
        }
    }

    private fun configurePreferencesPane() {
        val watcher = object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) = Unit
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) = Unit
            override fun afterTextChanged(s: Editable?) {
                preferencesDirty = true
            }
        }

        deviceNameInput.addTextChangedListener(watcher)
        historyLimitInput.addTextChangedListener(watcher)

        shareLocalSwitch.setOnCheckedChangeListener { _, _ -> preferencesDirty = true }
        preferRemoteSwitch.setOnCheckedChangeListener { _, _ -> preferencesDirty = true }
        discoverySwitch.setOnCheckedChangeListener { _, _ -> preferencesDirty = true }
        devicesText.setOnClickListener { showDevicePickerDialog() }
        trustDeviceButton.setOnClickListener { trustSelectedDevice() }
        revokeDeviceButton.setOnClickListener { revokeSelectedDeviceTrust() }

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
    }

    private fun submitCurrentClipboard() {
        val error = ClipboardInterop.submitCurrentClipboard(this, clipboardManager)
        if (error != null) {
            statusText.text = error
        }
    }

    private fun activateHistoryItem(itemId: String) {
        val result = RustBridge.activateHistoryItem(itemId)
        if (result.error != null) {
            statusText.text = result.error
            return
        }
        applyPendingClipboard(result.pendingClipboard)
        refreshSnapshot()
    }

    private fun refreshSnapshot() {
        val snapshot = RustBridge.refreshSnapshot(searchInput.text?.toString().orEmpty())
        if (snapshot.error != null) {
            statusText.text = snapshot.error
            return
        }

        adapter.submitList(snapshot.history)
        applyPendingClipboard(snapshot.pendingClipboard)
        snapshot.settings?.let { settings ->
            currentSettings = settings
            statusText.text = settings.status.orEmpty()
            renderDevices(settings.devices)
            if (!preferencesDirty) {
                populateSettingsForm(settings)
            }
        }
    }

    private fun populateSettingsForm(settings: RustBridge.SettingsSnapshot) {
        deviceNameInput.setText(settings.deviceName)
        historyLimitInput.setText(settings.historyLimit.toString())
        shareLocalSwitch.isChecked = settings.shareLocalHistory
        preferRemoteSwitch.isChecked = settings.preferRemoteLatestOnPaste
        discoverySwitch.isChecked = settings.discoveryEnabled
        preferencesDirty = false
    }

    private fun renderDevices(devices: List<RustBridge.SettingsDevice>) {
        currentDevices = devices
        if (selectedDeviceId != null && devices.none { it.deviceId == selectedDeviceId }) {
            selectedDeviceId = null
        }

        devicesText.text = devices
            .map { device ->
                val isSelected = device.deviceId == selectedDeviceId
                val selectedMark = if (isSelected) "• " else ""
                val trustState = if (device.isTrusted) "已信任" else "未信任"
                val onlineState = if (device.isOnline) "在线" else "离线"
                "${selectedMark}${device.deviceName}（${trustState} · ${onlineState}）\n${device.secondaryText}"
            }
            .joinToString("\n\n")
            .ifEmpty { getString(R.string.no_devices_yet) }

        refreshDeviceActionButtons()
    }

    private fun refreshDeviceActionButtons() {
        val selected = currentDevices.firstOrNull { it.deviceId == selectedDeviceId }
        trustDeviceButton.isEnabled = selected?.let { !it.isTrusted && it.isOnline } == true
        revokeDeviceButton.isEnabled = selected?.isTrusted == true
    }

    private fun showDevicePickerDialog() {
        if (currentDevices.isEmpty()) {
            Toast.makeText(this, getString(R.string.no_devices_yet), Toast.LENGTH_SHORT).show()
            return
        }

        val labels = currentDevices.map { device ->
            val trustState = if (device.isTrusted) "已信任" else "未信任"
            val onlineState = if (device.isOnline) "在线" else "离线"
            "${device.deviceName}（${trustState} · ${onlineState}）"
        }.toTypedArray()

        val currentIndex = currentDevices.indexOfFirst { it.deviceId == selectedDeviceId }
        var pendingIndex = currentIndex
        AlertDialog.Builder(this)
            .setTitle(getString(R.string.choose_device))
            .setSingleChoiceItems(labels, currentIndex) { _, which ->
                pendingIndex = which
            }
            .setPositiveButton(android.R.string.ok) { _, _ ->
                if (pendingIndex in currentDevices.indices) {
                    selectedDeviceId = currentDevices[pendingIndex].deviceId
                    renderDevices(currentDevices)
                }
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }

    private fun trustSelectedDevice() {
        val selectedDeviceId = selectedDeviceId
        if (selectedDeviceId == null) {
            statusText.text = getString(R.string.choose_device_first)
            return
        }

        val error = RustBridge.trustDevice(selectedDeviceId)
        if (error != null) {
            statusText.text = error
            return
        }
        refreshSnapshot()
    }

    private fun revokeSelectedDeviceTrust() {
        val selectedDeviceId = selectedDeviceId
        if (selectedDeviceId == null) {
            statusText.text = getString(R.string.choose_device_first)
            return
        }

        val error = RustBridge.revokeTrustedDevice(selectedDeviceId)
        if (error != null) {
            statusText.text = error
            return
        }
        refreshSnapshot()
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
            statusText.text = error
            return
        }

        preferencesDirty = false
        refreshSnapshot()
    }

    private fun applyPendingClipboard(payload: RustBridge.PendingClipboard?) {
        ClipboardInterop.applyPendingClipboard(this, clipboardManager, payload)
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
