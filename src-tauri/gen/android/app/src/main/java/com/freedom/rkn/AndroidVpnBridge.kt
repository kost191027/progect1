package com.freedom.rkn

import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.VpnService
import android.os.Build
import android.provider.Settings

object AndroidVpnBridge {
    @JvmStatic
    fun getNativeLibraryDir(context: Context): String =
        context.applicationInfo.nativeLibraryDir ?: ""

    @JvmStatic
    fun isVpnPermissionGranted(context: Context): Boolean = VpnService.prepare(context) == null

    @JvmStatic
    fun requestVpnPermission(activity: Activity): Boolean {
        val prepareIntent = VpnService.prepare(activity) ?: return true
        activity.startActivityForResult(prepareIntent, VPN_PERMISSION_REQUEST_CODE)
        return false
    }

    @JvmStatic
    fun startTunnelService(context: Context) {
        val intent = AndroidTunnelService.buildStartIntent(context)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            context.startForegroundService(intent)
        } else {
            context.startService(intent)
        }
    }

    @JvmStatic
    fun stopTunnelService(context: Context) {
        val intent = AndroidTunnelService.buildStopIntent(context)
        context.startService(intent)
    }

    @JvmStatic
    fun isTunnelInterfaceReady(context: Context): Boolean =
        AndroidTunnelService.isTunnelInterfaceReady()

    @JvmStatic
    fun getTunnelDebugState(context: Context): String = AndroidTunnelService.getTunnelDebugState()

    @JvmStatic
    fun peekTunnelFd(context: Context): Int = AndroidTunnelService.peekTunnelFd()

    @JvmStatic
    fun getTunnelAddress(context: Context): String = AndroidTunnelService.getTunnelAddress()

    @JvmStatic
    fun getTunnelPrefixLength(context: Context): Int = AndroidTunnelService.getTunnelPrefixLength()

    @JvmStatic
    fun getTunnelRoute(context: Context): String = AndroidTunnelService.getTunnelRoute()

    @JvmStatic
    fun getTunnelMtu(context: Context): Int = AndroidTunnelService.getTunnelMtu()

    @JvmStatic
    fun getPrivateDnsSummary(context: Context): String {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.P) {
            return "unsupported"
        }

        val mode = Settings.Global.getString(
            context.contentResolver,
            PRIVATE_DNS_MODE_KEY
        ) ?: "off"
        val host = Settings.Global.getString(
            context.contentResolver,
            PRIVATE_DNS_SPECIFIER_KEY
        ) ?: ""

        return if (host.isBlank()) mode else "$mode:$host"
    }

    @JvmStatic
    fun getActiveNetworkSummary(context: Context): String {
        val manager = context.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        val network = manager.activeNetwork ?: return "none"
        val capabilities = manager.getNetworkCapabilities(network) ?: return "unknown"
        val transports = mutableListOf<String>()

        if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI)) {
            transports.add("wifi")
        }
        if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR)) {
            transports.add("cellular")
        }
        if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_VPN)) {
            transports.add("vpn")
        }
        if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)) {
            transports.add("ethernet")
        }

        return if (transports.isEmpty()) "other" else transports.joinToString("+")
    }

    @JvmStatic
    fun registerBackendHandoffSession(
        context: Context,
        sessionId: String,
        contextPath: String,
        backendConfigPath: String,
        logPath: String,
        tunFd: Int
    ): String = AndroidTunnelService.registerBackendHandoffSession(
        sessionId = sessionId,
        contextPath = contextPath,
        backendConfigPath = backendConfigPath,
        logPath = logPath,
        tunFd = tunFd
    )

    @JvmStatic
    fun claimBackendHandoffSession(
        context: Context,
        sessionId: String,
        consumerTag: String
    ): String = AndroidTunnelService.claimBackendHandoffSession(
        sessionId = sessionId,
        consumerTag = consumerTag
    )

    @JvmStatic
    fun updateBackendHandoffSessionState(
        context: Context,
        sessionId: String,
        consumerTag: String,
        phase: String,
        detail: String
    ): String = AndroidTunnelService.updateBackendHandoffSessionState(
        sessionId = sessionId,
        consumerTag = consumerTag,
        phase = phase,
        detail = detail
    )

    @JvmStatic
    fun getBackendHandoffState(context: Context): String =
        AndroidTunnelService.getBackendHandoffState()

    @JvmStatic
    fun getBackendHandoffSessionId(context: Context): String =
        AndroidTunnelService.getBackendHandoffSessionId()

    @JvmStatic
    fun getBackendHandoffContextPath(context: Context): String =
        AndroidTunnelService.getBackendHandoffContextPath()

    @JvmStatic
    fun getBackendHandoffConfigPath(context: Context): String =
        AndroidTunnelService.getBackendHandoffConfigPath()

    @JvmStatic
    fun getBackendHandoffLogPath(context: Context): String =
        AndroidTunnelService.getBackendHandoffLogPath()

    @JvmStatic
    fun startNativeBackendSeam(context: Context, claimPath: String): String =
        AndroidNativeBackendSeam.startClaimedSession(context, claimPath)

    @JvmStatic
    fun abortNativeBackendSession(context: Context, sessionId: String, reason: String): String =
        AndroidNativeBackendSeam.abortClaimedSession(context, sessionId, reason)

    @JvmStatic
    fun getNativeBackendStatusPath(context: Context): String =
        AndroidNativeBackendSeam.getStatusPath(context)

    @JvmStatic
    fun getNativeBackendStatusState(context: Context): String =
        AndroidNativeBackendSeam.getStatusState(context)

    @JvmStatic
    fun protectSocketFd(context: Context, fd: Int): Boolean =
        AndroidTunnelService.protectSocketFd(fd)

    @JvmStatic
    fun writeClipboardText(context: Context, text: String) {
        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        clipboard.setPrimaryClip(ClipData.newPlainText("RKN", text))
    }

    @JvmStatic
    fun readClipboardText(context: Context): String {
        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        val clip = clipboard.primaryClip ?: return ""
        if (clip.itemCount <= 0) {
            return ""
        }

        return clip.getItemAt(0).coerceToText(context)?.toString() ?: ""
    }

    private const val VPN_PERMISSION_REQUEST_CODE = 6104
    private const val PRIVATE_DNS_MODE_KEY = "private_dns_mode"
    private const val PRIVATE_DNS_SPECIFIER_KEY = "private_dns_specifier"
}
