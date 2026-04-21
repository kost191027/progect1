package com.freedom.rkn

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.VpnService
import android.os.Build
import android.os.ParcelFileDescriptor
import android.os.Process
import android.os.IBinder
import androidx.core.app.NotificationCompat
import java.io.File

class AndroidTunnelService : VpnService() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        activeInstance = this

        when (intent?.action) {
            ACTION_STOP -> {
                stopRunningBackend()
                stopManagedCoreProcess()
                closeTunnelInterface()
                clearBackendHandoffSession()
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
                return START_NOT_STICKY
            }
            ACTION_START,
            null -> {
                ensureNotificationChannel()
                startForeground(NOTIFICATION_ID, buildNotification())
                ensureTunnelInterface()
                return START_STICKY
            }
            else -> {
                ensureNotificationChannel()
                startForeground(NOTIFICATION_ID, buildNotification())
                ensureTunnelInterface()
                return START_STICKY
            }
        }
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        // 6A.3 semantics: swiping the UI away must not silently kill the runtime anchor.
        super.onTaskRemoved(rootIntent)
    }

    override fun onRevoke() {
        stopRunningBackend()
        stopManagedCoreProcess()
        closeTunnelInterface()
        clearBackendHandoffSession()
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
        super.onRevoke()
    }

    override fun onDestroy() {
        if (activeInstance === this) {
            activeInstance = null
        }
        stopRunningBackend()
        closeTunnelInterface()
        clearBackendHandoffSession()
        super.onDestroy()
    }

    private fun buildNotification(): Notification {
        val openIntent = Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_NEW_TASK
            action = ACTION_OPEN_APP
        }

        val openPendingIntent = PendingIntent.getActivity(
            this,
            1001,
            openIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

        val stopIntent = Intent(this, AndroidTunnelService::class.java).apply {
            action = ACTION_STOP
        }

        val stopPendingIntent = PendingIntent.getService(
            this,
            1002,
            stopIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

        return NotificationCompat.Builder(this, NOTIFICATION_CHANNEL_ID)
            .setSmallIcon(R.mipmap.ic_launcher)
            .setContentTitle(getString(R.string.android_tunnel_notification_title))
            .setContentText(getString(R.string.android_tunnel_notification_text))
            .setContentIntent(openPendingIntent)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
            .addAction(
                0,
                getString(R.string.android_tunnel_notification_action_open),
                openPendingIntent,
            )
            .addAction(
                0,
                getString(R.string.android_tunnel_notification_action_stop),
                stopPendingIntent,
            )
            .build()
    }

    private fun ensureNotificationChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
            return
        }

        val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        val existing = manager.getNotificationChannel(NOTIFICATION_CHANNEL_ID)
        if (existing != null) {
            return
        }

        val channel = NotificationChannel(
            NOTIFICATION_CHANNEL_ID,
            getString(R.string.android_tunnel_notification_channel_name),
            NotificationManager.IMPORTANCE_LOW,
        ).apply {
            description = getString(R.string.android_tunnel_notification_channel_description)
            setShowBadge(false)
        }

        manager.createNotificationChannel(channel)
    }

    private fun stopManagedCoreProcess() {
        val pidFile = File(filesDir, "active_tunnel_pid")
        val pid = pidFile.takeIf { it.exists() }?.readText()?.trim()?.toIntOrNull() ?: return

        runCatching { Process.killProcess(pid) }
        runCatching { pidFile.delete() }
    }

    private fun ensureTunnelInterface() {
        synchronized(STATE_LOCK) {
            if (activeTunnelInterface != null) {
                lastTunnelState = buildTunnelStateSummary(activeTunnelInterface)
                lastTunnelError = null
                return
            }

            try {
                val descriptor = Builder()
                    .setSession(getString(R.string.app_name))
                    .setMtu(TUN_MTU)
                    .addAddress(TUN_ADDRESS, TUN_PREFIX_LENGTH)
                    .addRoute("0.0.0.0", 0)
                    .apply {
                        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                            setMetered(false)
                        }
                    }
                    .establish()
                    ?: throw IllegalStateException("VpnService.Builder.establish() returned null")

                activeTunnelInterface = descriptor
                lastTunnelState = buildTunnelStateSummary(descriptor)
                lastTunnelError = null
            } catch (error: Throwable) {
                lastTunnelError = error.message ?: error::class.java.simpleName
                lastTunnelState = "failed(${lastTunnelError})"
                closeTunnelInterface()
            }
        }
    }

    companion object {
        private const val NOTIFICATION_CHANNEL_ID = "rkn_tunnel_runtime"
        private const val NOTIFICATION_ID = 6103
        private const val TUN_ADDRESS = "172.19.0.1"
        private const val TUN_PREFIX_LENGTH = 30
        private const val TUN_MTU = 1500
        private val STATE_LOCK = Any()
        @Volatile
        private var activeTunnelInterface: ParcelFileDescriptor? = null
        @Volatile
        private var activeInstance: AndroidTunnelService? = null
        @Volatile
        private var lastTunnelError: String? = null
        @Volatile
        private var lastTunnelState: String = "idle"
        @Volatile
        private var backendHandoffState: String = "idle"
        @Volatile
        private var backendHandoffSessionId: String = ""
        @Volatile
        private var backendHandoffConsumerTag: String = ""
        @Volatile
        private var backendHandoffContextPath: String = ""
        @Volatile
        private var backendHandoffConfigPath: String = ""
        @Volatile
        private var backendHandoffLogPath: String = ""
        @Volatile
        private var activeBackendRunningHandle: AndroidNativeBackendRunningHandle? = null

        const val ACTION_START = "com.freedom.rkn.action.START_TUNNEL_SERVICE"
        const val ACTION_STOP = "com.freedom.rkn.action.STOP_TUNNEL_SERVICE"
        const val ACTION_OPEN_APP = "com.freedom.rkn.action.OPEN_APP_FROM_NOTIFICATION"

        fun buildStartIntent(context: Context): Intent {
            return Intent(context, AndroidTunnelService::class.java).apply {
                action = ACTION_START
            }
        }

        fun buildStopIntent(context: Context): Intent {
            return Intent(context, AndroidTunnelService::class.java).apply {
                action = ACTION_STOP
            }
        }

        fun isTunnelInterfaceReady(): Boolean {
            return activeTunnelInterface != null
        }

        fun getTunnelDebugState(): String {
            return lastTunnelError?.let { error ->
                "failed($error)"
            } ?: lastTunnelState
        }

        fun peekTunnelFd(): Int {
            return activeTunnelInterface?.fd ?: -1
        }

        fun getTunnelAddress(): String {
            return TUN_ADDRESS
        }

        fun getTunnelPrefixLength(): Int {
            return TUN_PREFIX_LENGTH
        }

        fun getTunnelRoute(): String {
            return "0.0.0.0/0"
        }

        fun getTunnelMtu(): Int {
            return TUN_MTU
        }

        fun registerBackendHandoffSession(
            sessionId: String,
            contextPath: String,
            backendConfigPath: String,
            logPath: String,
            tunFd: Int,
        ): String {
            synchronized(STATE_LOCK) {
                backendHandoffSessionId = sessionId
                backendHandoffContextPath = contextPath
                backendHandoffConfigPath = backendConfigPath
                backendHandoffLogPath = logPath
                backendHandoffState =
                    "pending(session=$sessionId, fd=$tunFd, context=$contextPath, backend_config=$backendConfigPath, log=$logPath)"
                return backendHandoffState
            }
        }

        fun claimBackendHandoffSession(sessionId: String, consumerTag: String): String {
            synchronized(STATE_LOCK) {
                if (backendHandoffSessionId.isEmpty()) {
                    return "missing"
                }

                if (backendHandoffSessionId != sessionId) {
                    return "session-mismatch(current=$backendHandoffSessionId, requested=$sessionId)"
                }

                backendHandoffConsumerTag = consumerTag
                backendHandoffState = "claimed(session=$sessionId, consumer=$consumerTag)"
                return backendHandoffState
            }
        }

        fun updateBackendHandoffSessionState(
            sessionId: String,
            consumerTag: String,
            phase: String,
            detail: String?,
        ): String {
            synchronized(STATE_LOCK) {
                if (backendHandoffSessionId.isEmpty()) {
                    return "missing"
                }

                if (backendHandoffSessionId != sessionId) {
                    return "session-mismatch(current=$backendHandoffSessionId, requested=$sessionId)"
                }

                if (backendHandoffConsumerTag.isNotEmpty() && backendHandoffConsumerTag != consumerTag) {
                    return "consumer-mismatch(current=$backendHandoffConsumerTag, requested=$consumerTag)"
                }

                backendHandoffConsumerTag = consumerTag
                backendHandoffState = if (detail.isNullOrBlank()) {
                    "$phase(session=$sessionId, consumer=$consumerTag)"
                } else {
                    "$phase(session=$sessionId, consumer=$consumerTag, detail=$detail)"
                }
                return backendHandoffState
            }
        }

        fun getBackendHandoffState(): String {
            return backendHandoffState
        }

        fun getBackendHandoffSessionId(): String {
            return backendHandoffSessionId
        }

        fun getBackendHandoffContextPath(): String {
            return backendHandoffContextPath
        }

        fun getBackendHandoffConfigPath(): String {
            return backendHandoffConfigPath
        }

        fun getBackendHandoffLogPath(): String {
            return backendHandoffLogPath
        }

        fun protectSocketFd(fd: Int): Boolean {
            val service = activeInstance ?: return false
            return runCatching { service.protect(fd) }.getOrDefault(false)
        }

        fun registerRunningBackend(handle: AndroidNativeBackendRunningHandle) {
            synchronized(STATE_LOCK) {
                activeBackendRunningHandle = handle
            }
        }

        fun stopRunningBackend(): String {
            synchronized(STATE_LOCK) {
                val handle = activeBackendRunningHandle ?: return "idle"
                val runtime = AndroidNativeBackendRuntimeRegistry.byId(handle.runtimeId)
                    ?: return "missing(runtime=${handle.runtimeId}, session=${handle.sessionId})"
                val state = runtime.stop(handle)
                activeBackendRunningHandle = null
                return state
            }
        }

        private fun buildTunnelStateSummary(descriptor: ParcelFileDescriptor?): String {
            val fd = descriptor?.fd ?: -1
            return "ready(fd=$fd, addr=$TUN_ADDRESS/$TUN_PREFIX_LENGTH, route=0.0.0.0/0, mtu=$TUN_MTU)"
        }

        private fun closeTunnelInterface() {
            synchronized(STATE_LOCK) {
                activeTunnelInterface?.let { descriptor ->
                    runCatching { descriptor.close() }
                }
                activeTunnelInterface = null
                if (lastTunnelError == null) {
                    lastTunnelState = "idle"
                }
            }
        }

        private fun clearBackendHandoffSession() {
            synchronized(STATE_LOCK) {
                activeBackendRunningHandle = null
                backendHandoffState = "idle"
                backendHandoffSessionId = ""
                backendHandoffConsumerTag = ""
                backendHandoffContextPath = ""
                backendHandoffConfigPath = ""
                backendHandoffLogPath = ""
            }
        }
    }
}
