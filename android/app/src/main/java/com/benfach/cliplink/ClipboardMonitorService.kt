package com.benfach.cliplink

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.os.IBinder
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import java.util.concurrent.atomic.AtomicBoolean

class ClipboardMonitorService : Service() {
    private lateinit var clipboardManager: ClipboardManager
    private lateinit var workerThread: HandlerThread
    private lateinit var workerHandler: Handler
    private var multicastLock: WifiManager.MulticastLock? = null
    private var listenerRegistered = false
    private var lastReportedRuntimeError: String? = null
    private val clipboardDirty = AtomicBoolean(true)
    private var idleTickCount = 0

    private val clipboardListener = ClipboardManager.OnPrimaryClipChangedListener {
        clipboardDirty.set(true)
        runTickNow()
    }

    private val tickRunnable = object : Runnable {
        override fun run() {
            val nextDelayMs = tickRuntime()
            scheduleNextTick(nextDelayMs)
        }
    }

    override fun onCreate() {
        super.onCreate()
        clipboardManager = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        workerThread = HandlerThread("cliplink-runtime")
        workerThread.start()
        workerHandler = Handler(workerThread.looper)
        startForeground(NOTIFICATION_ID, buildNotification())
        acquireMulticastLock()
        bootstrapBridge()
        registerClipboardListener()
        runTickNow()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        return START_STICKY
    }

    override fun onDestroy() {
        super.onDestroy()
        if (listenerRegistered) {
            clipboardManager.removePrimaryClipChangedListener(clipboardListener)
            listenerRegistered = false
        }
        if (::workerHandler.isInitialized) {
            workerHandler.removeCallbacks(tickRunnable)
        }
        if (::workerThread.isInitialized) {
            workerThread.quitSafely()
        }
        releaseMulticastLock()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun bootstrapBridge() {
        RustBridge.bootstrap(this)
    }

    private fun registerClipboardListener() {
        if (listenerRegistered) {
            return
        }
        clipboardManager.addPrimaryClipChangedListener(clipboardListener)
        listenerRegistered = true
    }

    private fun runTickNow() {
        if (!::workerHandler.isInitialized) {
            return
        }
        workerHandler.removeCallbacks(tickRunnable)
        workerHandler.post(tickRunnable)
    }

    private fun scheduleNextTick(delayMs: Long) {
        workerHandler.removeCallbacks(tickRunnable)
        workerHandler.postDelayed(tickRunnable, delayMs.coerceAtLeast(0L))
    }

    private fun tickRuntime(): Long {
        val clipboardSubmission = if (clipboardDirty.compareAndSet(true, false)) {
            ClipboardInterop.submitCurrentClipboard(this, clipboardManager)
        } else {
            ClipboardInterop.ClipboardSubmissionResult(submitted = false, error = null)
        }
        val result = RustBridge.tick()
        val runtimeError = clipboardSubmission.error ?: result.error
        ClipboardInterop.applyPendingClipboard(this, clipboardManager, result.pendingClipboard)
        val errorChanged = runtimeError != lastReportedRuntimeError
        if (errorChanged) {
            lastReportedRuntimeError = runtimeError
        }
        val localHistoryChanged = clipboardSubmission.submitted
        val clipboardAwaitingForeground = clipboardSubmission.requiresForegroundAccess
        val hasVisibleActivity =
            localHistoryChanged ||
                result.pendingClipboard != null ||
                result.historyChanged ||
                result.devicesChanged ||
                result.statusChanged ||
                result.transferChanged ||
                errorChanged
        idleTickCount = if (hasVisibleActivity) {
            0
        } else {
            (idleTickCount + 1).coerceAtMost(MAX_IDLE_TICK_COUNT)
        }
        if (
            localHistoryChanged ||
            result.pendingClipboard != null ||
            result.historyChanged ||
            result.devicesChanged ||
            result.statusChanged ||
            result.transferChanged ||
            errorChanged
        ) {
            notifyStateChanged(runtimeError)
        }
        return when {
            hasVisibleActivity -> ACTIVE_TICK_INTERVAL_MS
            clipboardAwaitingForeground -> QUIET_TICK_INTERVAL_MS
            idleTickCount <= 2 -> QUIET_TICK_INTERVAL_MS
            else -> IDLE_TICK_INTERVAL_MS
        }
    }

    private fun notifyStateChanged(runtimeError: String? = null) {
        val intent = Intent(ACTION_STATE_CHANGED).setPackage(packageName)
        if (!runtimeError.isNullOrBlank()) {
            intent.putExtra(EXTRA_RUNTIME_ERROR, runtimeError)
        }
        sendBroadcast(intent)
    }

    private fun acquireMulticastLock() {
        val wifiManager = applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager
        if (wifiManager == null) {
            return
        }
        if (multicastLock == null) {
            multicastLock = wifiManager.createMulticastLock("cliplink-discovery").apply {
                setReferenceCounted(false)
            }
        }
        multicastLock?.acquire()
    }

    private fun releaseMulticastLock() {
        multicastLock?.let { lock ->
            if (lock.isHeld) {
                lock.release()
            }
        }
    }

    private fun buildNotification(): Notification {
        ensureNotificationChannel()
        val launchIntent = Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
        }
        val pendingIntent = PendingIntent.getActivity(
            this,
            0,
            launchIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

        return NotificationCompat.Builder(this, NOTIFICATION_CHANNEL_ID)
            .setContentTitle(getString(R.string.android_service_notification_title))
            .setContentText(getString(R.string.android_service_notification_text))
            .setSmallIcon(android.R.drawable.stat_notify_sync)
            .setOngoing(true)
            .setShowWhen(false)
            .setContentIntent(pendingIntent)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .build()
    }

    private fun ensureNotificationChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
            return
        }

        val manager = getSystemService(NotificationManager::class.java) ?: return
        val channel = NotificationChannel(
            NOTIFICATION_CHANNEL_ID,
            getString(R.string.android_service_notification_channel),
            NotificationManager.IMPORTANCE_LOW,
        ).apply {
            description = getString(R.string.android_service_notification_channel_description)
            setShowBadge(false)
        }
        manager.createNotificationChannel(channel)
    }

    companion object {
        const val ACTION_STATE_CHANGED = "com.benfach.cliplink.ACTION_STATE_CHANGED"
        const val EXTRA_RUNTIME_ERROR = "com.benfach.cliplink.EXTRA_RUNTIME_ERROR"

        private const val NOTIFICATION_CHANNEL_ID = "cliplink.background.sync"
        private const val NOTIFICATION_ID = 1001
        private const val ACTIVE_TICK_INTERVAL_MS = 120L
        private const val QUIET_TICK_INTERVAL_MS = 320L
        private const val IDLE_TICK_INTERVAL_MS = 720L
        private const val MAX_IDLE_TICK_COUNT = 8

        fun start(context: Context) {
            val intent = Intent(context, ClipboardMonitorService::class.java)
            ContextCompat.startForegroundService(context, intent)
        }
    }
}
