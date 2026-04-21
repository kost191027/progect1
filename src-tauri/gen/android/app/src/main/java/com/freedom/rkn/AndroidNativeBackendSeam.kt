package com.freedom.rkn

import android.content.Context
import org.json.JSONObject
import java.io.File

object AndroidNativeBackendSeam {
    private const val DEFAULT_CONSUMER_TAG = "rkn_android_native_backend_seam"
    private const val STATUS_FILE_NAME = "android_native_backend_status.json"

    @JvmStatic
    fun startClaimedSession(context: Context, launchBundlePath: String): String {
        var sessionId = ""
        var consumerTag = DEFAULT_CONSUMER_TAG
        var statusFile = File(context.filesDir, STATUS_FILE_NAME)

        fun persistStatus(
            phase: String,
            detail: String,
            tunFd: Int = -1,
            contextPath: String = "",
            backendConfigPath: String = "",
            logPath: String = "",
        ): String {
            val payload = JSONObject().apply {
                put("phase", phase)
                put("detail", detail)
                put("session_id", sessionId)
                put("consumer_tag", consumerTag)
                put("launch_bundle_path", launchBundlePath)
                put("status_path", statusFile.absolutePath)
                put("tun_fd", tunFd)
                put("context_path", contextPath)
                put("backend_config_path", backendConfigPath)
                put("log_path", logPath)
            }
            runCatching { statusFile.writeText(payload.toString(2)) }
            return payload.toString()
        }

        return try {
            val launchBundleFile = File(launchBundlePath)
            if (!launchBundleFile.exists()) {
                return persistStatus(
                    phase = "failed",
                    detail = "Android native backend seam could not find the launch bundle artifact.",
                )
            }

            val launchBundle = AndroidNativeBackendLaunchBundlePayload.fromFile(launchBundlePath)
            sessionId = launchBundle.sessionId
            consumerTag = launchBundle.consumerTag
            statusFile = File(launchBundle.runtimeStatusPath)
            statusFile.parentFile?.mkdirs()

            if (sessionId.isBlank()) {
                return persistStatus(
                    phase = "failed",
                    detail = "Android native backend seam received an empty handoff session id from the launch bundle.",
                    tunFd = launchBundle.tunFd,
                    contextPath = launchBundle.contextPath,
                    backendConfigPath = launchBundle.backendConfigPath,
                    logPath = launchBundle.logPath,
                )
            }

            AndroidTunnelService.updateBackendHandoffSessionState(
                sessionId = sessionId,
                consumerTag = consumerTag,
                phase = "launching",
                detail = "Android native backend seam is validating the claimed handoff session.",
            )

            val contextFile = File(launchBundle.contextPath)
            val backendConfigFile = File(launchBundle.backendConfigPath)
            val logFile = File(launchBundle.logPath)
            logFile.parentFile?.mkdirs()
            File(launchBundle.sessionDir).mkdirs()
            File(launchBundle.runtimeLogPath).parentFile?.mkdirs()

            if (launchBundle.tunFd <= 0) {
                val detail =
                    "Android native backend seam received a non-ready TUN fd from VpnService."
                AndroidTunnelService.updateBackendHandoffSessionState(
                    sessionId = sessionId,
                    consumerTag = consumerTag,
                    phase = "failed",
                    detail = detail,
                )
                return persistStatus(
                    phase = "failed",
                    detail = detail,
                    tunFd = launchBundle.tunFd,
                    contextPath = launchBundle.contextPath,
                    backendConfigPath = launchBundle.backendConfigPath,
                    logPath = launchBundle.logPath,
                )
            }

            if (launchBundle.backendHint != "android_native_handoff_required") {
                val detail =
                    "Android native backend seam received a launch bundle with an unexpected backend hint."
                AndroidTunnelService.updateBackendHandoffSessionState(
                    sessionId = sessionId,
                    consumerTag = consumerTag,
                    phase = "failed",
                    detail = detail,
                )
                return persistStatus(
                    phase = "failed",
                    detail = detail,
                    tunFd = launchBundle.tunFd,
                    contextPath = launchBundle.contextPath,
                    backendConfigPath = launchBundle.backendConfigPath,
                    logPath = launchBundle.logPath,
                )
            }

            if (!contextFile.exists() || !backendConfigFile.exists()) {
                val detail =
                    "Android native backend seam could not find the handoff context or backend config artifact from the launch bundle."
                AndroidTunnelService.updateBackendHandoffSessionState(
                    sessionId = sessionId,
                    consumerTag = consumerTag,
                    phase = "failed",
                    detail = detail,
                )
                return persistStatus(
                    phase = "failed",
                    detail = detail,
                    tunFd = launchBundle.tunFd,
                    contextPath = launchBundle.contextPath,
                    backendConfigPath = launchBundle.backendConfigPath,
                    logPath = launchBundle.logPath,
                )
            }

            val selection = AndroidNativeBackendRuntimeRegistry.current(context, launchBundle)
            val launchResult = selection.runtime.launch(context, launchBundle).copy(
                runtimeSelection = selection.selectionSummary,
            )
            launchResult.runningHandle?.let { handle ->
                AndroidTunnelService.registerRunningBackend(handle)
            }
            val launchState = AndroidTunnelService.updateBackendHandoffSessionState(
                sessionId = sessionId,
                consumerTag = consumerTag,
                phase = launchResult.phase,
                detail = launchResult.detail,
            )
            val payload = launchResult.toJson(
                launchBundlePath = launchBundlePath,
                statusPath = statusFile.absolutePath,
                bundle = launchBundle,
            ).apply {
                put("launch_state", launchState)
            }
            runCatching { statusFile.writeText(payload.toString(2)) }
            payload.toString()
        } catch (error: Throwable) {
            val detail = error.message ?: error::class.java.simpleName
            if (sessionId.isNotBlank()) {
                AndroidTunnelService.updateBackendHandoffSessionState(
                    sessionId = sessionId,
                    consumerTag = consumerTag,
                    phase = "failed",
                    detail = detail,
                )
            }
            persistStatus(
                phase = "failed",
                detail = "Android native backend seam crashed while preparing the launch bundle: $detail",
            )
        }
    }

    @JvmStatic
    fun getStatusPath(context: Context): String {
        return File(context.filesDir, STATUS_FILE_NAME).absolutePath
    }

    @JvmStatic
    fun getStatusState(context: Context): String {
        val statusFile = File(context.filesDir, STATUS_FILE_NAME)
        if (!statusFile.exists()) {
            return "idle"
        }

        return runCatching {
            val payload = JSONObject(statusFile.readText())
            val phase = payload.optString("phase", "unknown")
            val detail = payload.optString("detail")
            if (detail.isBlank()) {
                phase
            } else {
                "$phase(detail=$detail)"
            }
        }.getOrElse { error ->
            "failed(detail=${error.message ?: error::class.java.simpleName})"
        }
    }
}
