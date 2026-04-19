package com.freedom.rkn

import android.app.Activity
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

    private const val VPN_PERMISSION_REQUEST_CODE = 6104
}
