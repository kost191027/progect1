package com.freedom.rkn

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager.NameNotFoundException
import android.net.VpnService
import android.os.Build
import android.os.IBinder
import android.os.ParcelFileDescriptor
import android.os.Process
import android.os.SystemClock
import androidx.core.app.NotificationCompat
import io.nekohasekai.libbox.RoutePrefix
import io.nekohasekai.libbox.RoutePrefixIterator
import io.nekohasekai.libbox.TunOptions
import java.io.File

class AndroidTunnelService : VpnService() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        activeInstance = this

        when (intent?.action) {
            ACTION_STOP -> {
                stopRunningBackendAndDrain()
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
                return START_STICKY
            }
            else -> {
                ensureNotificationChannel()
                startForeground(NOTIFICATION_ID, buildNotification())
                return START_STICKY
            }
        }
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        // 6A.3 semantics: swiping the UI away must not silently kill the runtime anchor.
        super.onTaskRemoved(rootIntent)
    }

    override fun onRevoke() {
        stopRunningBackendAndDrain()
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
        stopRunningBackendAndDrain()
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
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        val stopIntent = Intent(this, AndroidTunnelService::class.java).apply {
            action = ACTION_STOP
        }

        val stopPendingIntent = PendingIntent.getService(
            this,
            1002,
            stopIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
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
                openPendingIntent
            )
            .addAction(
                0,
                getString(R.string.android_tunnel_notification_action_stop),
                stopPendingIntent
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
            NotificationManager.IMPORTANCE_LOW
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

    private fun stopRunningBackendAndDrain() {
        val state = stopRunningBackend()
        if (!state.startsWith("idle") && !state.startsWith("missing")) {
            SystemClock.sleep(150)
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

        @Volatile
        private var currentTunAddress: String = TUN_ADDRESS

        @Volatile
        private var currentTunPrefixLength: Int = TUN_PREFIX_LENGTH

        @Volatile
        private var currentTunRouteSummary: String = "0.0.0.0/0"

        @Volatile
        private var currentTunMtu: Int = TUN_MTU

        const val ACTION_START = "com.freedom.rkn.action.START_TUNNEL_SERVICE"
        const val ACTION_STOP = "com.freedom.rkn.action.STOP_TUNNEL_SERVICE"
        const val ACTION_OPEN_APP = "com.freedom.rkn.action.OPEN_APP_FROM_NOTIFICATION"

        fun buildStartIntent(context: Context): Intent =
            Intent(context, AndroidTunnelService::class.java).apply {
                action = ACTION_START
            }

        fun buildStopIntent(context: Context): Intent =
            Intent(context, AndroidTunnelService::class.java).apply {
                action = ACTION_STOP
            }

        fun isTunnelInterfaceReady(): Boolean = activeTunnelInterface != null

        fun getTunnelDebugState(): String = lastTunnelError?.let { error ->
            "failed($error)"
        } ?: lastTunnelState

        fun peekTunnelFd(): Int = activeTunnelInterface?.fd ?: -1

        fun openTunnelInterface(options: TunOptions): ParcelFileDescriptor? {
            synchronized(STATE_LOCK) {
                val service = activeInstance ?: return null
                return runCatching {
                    activeTunnelInterface?.let { existing ->
                        runCatching { existing.close() }
                        activeTunnelInterface = null
                    }
                    val descriptor = service.establishTunnelInterfaceLocked(options)
                    activeTunnelInterface = descriptor
                    lastTunnelState = buildTunnelStateSummary(descriptor)
                    lastTunnelError = null
                    ParcelFileDescriptor.dup(descriptor.fileDescriptor)
                }.getOrNull()
            }
        }

        fun getTunnelAddress(): String = currentTunAddress

        fun getTunnelPrefixLength(): Int = currentTunPrefixLength

        fun getTunnelRoute(): String = currentTunRouteSummary

        fun getTunnelMtu(): Int = currentTunMtu

        fun registerBackendHandoffSession(
            sessionId: String,
            contextPath: String,
            backendConfigPath: String,
            logPath: String,
            tunFd: Int
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
            detail: String?
        ): String {
            synchronized(STATE_LOCK) {
                if (backendHandoffSessionId.isEmpty()) {
                    return "missing"
                }

                if (backendHandoffSessionId != sessionId) {
                    return "session-mismatch(current=$backendHandoffSessionId, requested=$sessionId)"
                }

                if (backendHandoffConsumerTag.isNotEmpty() &&
                    backendHandoffConsumerTag != consumerTag
                ) {
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

        fun getBackendHandoffState(): String = backendHandoffState

        fun getBackendHandoffSessionId(): String = backendHandoffSessionId

        fun getBackendHandoffContextPath(): String = backendHandoffContextPath

        fun getBackendHandoffConfigPath(): String = backendHandoffConfigPath

        fun getBackendHandoffLogPath(): String = backendHandoffLogPath

        fun protectSocketFd(fd: Int): Boolean {
            val service = activeInstance ?: return false
            return runCatching { service.protect(fd) }.getOrDefault(false)
        }

        fun registerRunningBackend(handle: AndroidNativeBackendRunningHandle) {
            synchronized(STATE_LOCK) {
                activeBackendRunningHandle = handle
            }
        }

        fun clearRunningBackend(handle: AndroidNativeBackendRunningHandle) {
            synchronized(STATE_LOCK) {
                if (activeBackendRunningHandle == handle) {
                    activeBackendRunningHandle = null
                }
            }
        }

        fun abortBackendLaunch(sessionId: String, reason: String): String {
            val stopState = stopRunningBackend()
            synchronized(STATE_LOCK) {
                if (backendHandoffSessionId.isNotEmpty() && backendHandoffSessionId != sessionId) {
                    return "session-mismatch(current=$backendHandoffSessionId, requested=$sessionId, stop=$stopState)"
                }

                backendHandoffState =
                    "failed(session=$sessionId, detail=Android backend launch aborted: $reason)"
                backendHandoffSessionId = ""
                backendHandoffConsumerTag = ""
                backendHandoffContextPath = ""
                backendHandoffConfigPath = ""
                backendHandoffLogPath = ""
            }
            closeTunnelInterface()
            return "aborted(session=$sessionId, stop=$stopState)"
        }

        fun stopRunningBackend(): String {
            val (handle, runtime) = synchronized(STATE_LOCK) {
                val currentHandle = activeBackendRunningHandle ?: return "idle"
                val currentRuntime = AndroidNativeBackendRuntimeRegistry.byId(
                    currentHandle.runtimeId
                )
                if (currentRuntime == null) {
                    activeBackendRunningHandle = null
                    return "missing(runtime=${currentHandle.runtimeId}, session=${currentHandle.sessionId})"
                }
                currentHandle to currentRuntime
            }

            val state = runtime.stop(handle)

            synchronized(STATE_LOCK) {
                if (activeBackendRunningHandle == handle) {
                    activeBackendRunningHandle = null
                }
            }

            return state
        }

        private fun buildTunnelStateSummary(descriptor: ParcelFileDescriptor?): String {
            val fd = descriptor?.fd ?: -1
            return "ready(fd=$fd, addr=$currentTunAddress/$currentTunPrefixLength, route=$currentTunRouteSummary, mtu=$currentTunMtu)"
        }

        private fun closeTunnelInterface() {
            synchronized(STATE_LOCK) {
                activeTunnelInterface?.let { descriptor ->
                    runCatching { descriptor.close() }
                }
                activeTunnelInterface = null
                currentTunAddress = TUN_ADDRESS
                currentTunPrefixLength = TUN_PREFIX_LENGTH
                currentTunRouteSummary = "0.0.0.0/0"
                currentTunMtu = TUN_MTU
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

    private fun establishTunnelInterfaceLocked(options: TunOptions?): ParcelFileDescriptor {
        val mtu = options?.getMTU()?.takeIf { it > 0 } ?: TUN_MTU
        val inet4Address = options?.getInet4Address().toRoutePrefixList()
        val inet6Address = options?.getInet6Address().toRoutePrefixList()
        val dnsServer = runCatching {
            options?.getDNSServerAddress()?.value
        }.getOrNull()?.takeIf { !it.isNullOrBlank() }
        val autoRoute = options?.getAutoRoute() ?: true

        val builder = Builder()
            .setSession(getString(R.string.app_name))
            .setMtu(mtu)
            .apply {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    setMetered(false)
                }
            }

        if (inet4Address.isEmpty() && inet6Address.isEmpty()) {
            builder.addAddress(TUN_ADDRESS, TUN_PREFIX_LENGTH)
        } else {
            inet4Address.forEach { builder.addAddress(it.address(), it.prefix()) }
            inet6Address.forEach { builder.addAddress(it.address(), it.prefix()) }
        }

        if (autoRoute) {
            dnsServer?.let { builder.addDnsServer(it) }

            // Product mode is a full-device VPN. Do not honor include-package,
            // exclude-package, or route-exclude hints here: those can let games and native apps
            // bypass the tunnel. sing-box still decides direct vs proxy after
            // packets enter the VpnService-owned TUN, and outbound sockets are
            // protected through PlatformInterface.protect().
            builder.addRoute("0.0.0.0", 0)
            if (inet6Address.isNotEmpty()) {
                builder.addRoute("::", 0)
            }

            listOf(packageName).forEach { blockedPackage ->
                try {
                    builder.addDisallowedApplication(blockedPackage)
                } catch (_: NameNotFoundException) {
                }
            }
        } else {
            builder.addRoute("0.0.0.0", 0)
        }

        val descriptor = builder.establish()
            ?: throw IllegalStateException("VpnService.Builder.establish() returned null")

        val firstAddress = inet4Address.firstOrNull() ?: inet6Address.firstOrNull()
        currentTunAddress = firstAddress?.address() ?: TUN_ADDRESS
        currentTunPrefixLength = firstAddress?.prefix() ?: TUN_PREFIX_LENGTH
        currentTunRouteSummary = "0.0.0.0/0"
        currentTunMtu = mtu

        return descriptor
    }

    private fun RoutePrefixIterator?.toRoutePrefixList(): List<RoutePrefix> {
        if (this == null) {
            return emptyList()
        }

        val values = mutableListOf<RoutePrefix>()
        while (hasNext()) {
            values += next()
        }
        return values
    }
}
