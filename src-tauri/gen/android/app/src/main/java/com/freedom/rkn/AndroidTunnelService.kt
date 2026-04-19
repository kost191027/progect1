package com.freedom.rkn

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.VpnService
import android.os.Build
import android.os.Process
import android.os.IBinder
import androidx.core.app.NotificationCompat
import java.io.File

class AndroidTunnelService : VpnService() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                stopManagedCoreProcess()
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
        stopManagedCoreProcess()
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
        super.onRevoke()
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

    companion object {
        private const val NOTIFICATION_CHANNEL_ID = "rkn_tunnel_runtime"
        private const val NOTIFICATION_ID = 6103

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
    }
}
