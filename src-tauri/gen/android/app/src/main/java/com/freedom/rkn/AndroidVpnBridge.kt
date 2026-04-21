package com.freedom.rkn

import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.VpnService
import android.os.Build

object AndroidVpnBridge {
    @JvmStatic
    fun getNativeLibraryDir(context: Context): String {
        return context.applicationInfo.nativeLibraryDir ?: ""
    }

    @JvmStatic
    fun isVpnPermissionGranted(context: Context): Boolean {
        return VpnService.prepare(context) == null
    }

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
    fun isTunnelInterfaceReady(context: Context): Boolean {
        return AndroidTunnelService.isTunnelInterfaceReady()
    }

    @JvmStatic
    fun getTunnelDebugState(context: Context): String {
        return AndroidTunnelService.getTunnelDebugState()
    }

    @JvmStatic
    fun peekTunnelFd(context: Context): Int {
        return AndroidTunnelService.peekTunnelFd()
    }

    @JvmStatic
    fun getTunnelAddress(context: Context): String {
        return AndroidTunnelService.getTunnelAddress()
    }

    @JvmStatic
    fun getTunnelPrefixLength(context: Context): Int {
        return AndroidTunnelService.getTunnelPrefixLength()
    }

    @JvmStatic
    fun getTunnelRoute(context: Context): String {
        return AndroidTunnelService.getTunnelRoute()
    }

    @JvmStatic
    fun getTunnelMtu(context: Context): Int {
        return AndroidTunnelService.getTunnelMtu()
    }

    @JvmStatic
    fun registerBackendHandoffSession(
        context: Context,
        sessionId: String,
        contextPath: String,
        backendConfigPath: String,
        logPath: String,
        tunFd: Int,
    ): String {
        return AndroidTunnelService.registerBackendHandoffSession(
            sessionId = sessionId,
            contextPath = contextPath,
            backendConfigPath = backendConfigPath,
            logPath = logPath,
            tunFd = tunFd,
        )
    }

    @JvmStatic
    fun claimBackendHandoffSession(
        context: Context,
        sessionId: String,
        consumerTag: String,
    ): String {
        return AndroidTunnelService.claimBackendHandoffSession(
            sessionId = sessionId,
            consumerTag = consumerTag,
        )
    }

    @JvmStatic
    fun updateBackendHandoffSessionState(
        context: Context,
        sessionId: String,
        consumerTag: String,
        phase: String,
        detail: String,
    ): String {
        return AndroidTunnelService.updateBackendHandoffSessionState(
            sessionId = sessionId,
            consumerTag = consumerTag,
            phase = phase,
            detail = detail,
        )
    }

    @JvmStatic
    fun getBackendHandoffState(context: Context): String {
        return AndroidTunnelService.getBackendHandoffState()
    }

    @JvmStatic
    fun getBackendHandoffSessionId(context: Context): String {
        return AndroidTunnelService.getBackendHandoffSessionId()
    }

    @JvmStatic
    fun getBackendHandoffContextPath(context: Context): String {
        return AndroidTunnelService.getBackendHandoffContextPath()
    }

    @JvmStatic
    fun getBackendHandoffConfigPath(context: Context): String {
        return AndroidTunnelService.getBackendHandoffConfigPath()
    }

    @JvmStatic
    fun getBackendHandoffLogPath(context: Context): String {
        return AndroidTunnelService.getBackendHandoffLogPath()
    }

    @JvmStatic
    fun startNativeBackendSeam(context: Context, claimPath: String): String {
        return AndroidNativeBackendSeam.startClaimedSession(context, claimPath)
    }

    @JvmStatic
    fun getNativeBackendStatusPath(context: Context): String {
        return AndroidNativeBackendSeam.getStatusPath(context)
    }

    @JvmStatic
    fun getNativeBackendStatusState(context: Context): String {
        return AndroidNativeBackendSeam.getStatusState(context)
    }

    @JvmStatic
    fun protectSocketFd(context: Context, fd: Int): Boolean {
        return AndroidTunnelService.protectSocketFd(fd)
    }

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
}
