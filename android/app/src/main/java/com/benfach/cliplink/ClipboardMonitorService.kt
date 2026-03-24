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
import android.os.IBinder
import android.os.Looper
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat

class ClipboardMonitorService : Service() {
    private lateinit var clipboardManager: ClipboardManager
    private val mainHandler = Handler(Looper.getMainLooper())
    private var multicastLock: WifiManager.MulticastLock? = null
    private var listenerRegistered = false

    private val clipboardListener = ClipboardManager.OnPrimaryClipChangedListener {
        ClipboardInterop.submitCurrentClipboard(this, clipboardManager)
        notifyStateChanged()
        tickRuntime()
    }

    private val tickRunnable = object : Runnable {
        override fun run() {
            tickRuntime()
            mainHandler.postDelayed(this, SERVICE_TICK_INTERVAL_MS)
        }
    }

    override fun onCreate() {
        super.onCreate()
        clipboardManager = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        startForeground(NOTIFICATION_ID, buildNotification())
        bootstrapBridge()
        registerClipboardListener()
        acquireMulticastLock()
        ClipboardInterop.submitCurrentClipboard(this, clipboardManager)
        tickRuntime()
        mainHandler.post(tickRunnable)
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
        mainHandler.removeCallbacks(tickRunnable)
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

    private fun tickRuntime() {
        val result = RustBridge.tick()
        ClipboardInterop.applyPendingClipboard(this, clipboardManager, result.pendingClipboard)
        if (
            result.pendingClipboard != null ||
            result.historyChanged ||
            result.devicesChanged ||
            result.statusChanged ||
            result.error != null
        ) {
            notifyStateChanged()
        }
    }

    private fun notifyStateChanged() {
        sendBroadcast(Intent(ACTION_STATE_CHANGED).setPackage(packageName))
    }

    private fun acquireMulticastLock() {
        val wifiManager = applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager
        if (wifiManager == null) {
            return
        }
        if (multicastLock == null) {
            multicastLock = wifiManager.createMulticastLock("cliplink-mdns").apply {
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

        private const val NOTIFICATION_CHANNEL_ID = "cliplink.background.sync"
        private const val NOTIFICATION_ID = 1001
        private const val SERVICE_TICK_INTERVAL_MS = 1200L

        fun start(context: Context) {
            val intent = Intent(context, ClipboardMonitorService::class.java)
            ContextCompat.startForegroundService(context, intent)
        }
    }
}
