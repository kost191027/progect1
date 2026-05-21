package com.freedom.rkn

import android.content.Context
import java.io.File
import org.json.JSONObject

object AndroidNativeBackendSeam {
    private const val DEFAULT_CONSUMER_TAG = "rkn_android_native_backend_seam"
    private const val STATUS_FILE_NAME = "android_native_backend_status.json"

    private fun globalStatusFile(context: Context): File = File(context.filesDir, STATUS_FILE_NAME)

    @JvmStatic
    fun startClaimedSession(context: Context, launchBundlePath: String): String {
        var sessionId = ""
        var consumerTag = DEFAULT_CONSUMER_TAG
        var statusFile = globalStatusFile(context)

        fun persistStatus(
            phase: String,
            detail: String,
            tunFd: Int = -1,
            contextPath: String = "",
            backendConfigPath: String = "",
            logPath: String = ""
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
            runCatching {
                statusFile.parentFile?.mkdirs()
                statusFile.writeText(payload.toString(2))
                val global = globalStatusFile(context)
                global.parentFile?.mkdirs()
                if (global.absolutePath != statusFile.absolutePath) {
                    global.writeText(payload.toString(2))
                }
            }
            return payload.toString()
        }

        return try {
            val launchBundleFile = File(launchBundlePath)
            if (!launchBundleFile.exists()) {
                return persistStatus(
                    phase = "failed",
                    detail = "Android native backend seam could not find the launch bundle artifact."
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
                    logPath = launchBundle.logPath
                )
            }

            if (launchBundle.backendHint == "android_native_handoff_required") {
                AndroidTunnelService.updateBackendHandoffSessionState(
                    sessionId = sessionId,
                    consumerTag = consumerTag,
                    phase = "launching",
                    detail = "Android native backend seam is validating the claimed handoff session."
                )
            }

            val contextFile = File(launchBundle.contextPath)
            val backendConfigFile = File(launchBundle.backendConfigPath)
            val logFile = File(launchBundle.logPath)
            logFile.parentFile?.mkdirs()
            File(launchBundle.sessionDir).mkdirs()
            File(launchBundle.runtimeLogPath).parentFile?.mkdirs()

            if (
                launchBundle.backendHint != "android_native_handoff_required" &&
                launchBundle.backendHint != "android_native_proxy_fallback"
            ) {
                val detail =
                    "Android native backend seam received a launch bundle with an unexpected backend hint."
                AndroidTunnelService.updateBackendHandoffSessionState(
                    sessionId = sessionId,
                    consumerTag = consumerTag,
                    phase = "failed",
                    detail = detail
                )
                return persistStatus(
                    phase = "failed",
                    detail = detail,
                    tunFd = launchBundle.tunFd,
                    contextPath = launchBundle.contextPath,
                    backendConfigPath = launchBundle.backendConfigPath,
                    logPath = launchBundle.logPath
                )
            }

            if (!contextFile.exists() || !backendConfigFile.exists()) {
                val detail =
                    "Android native backend seam could not find the handoff context or backend config artifact from the launch bundle."
                AndroidTunnelService.updateBackendHandoffSessionState(
                    sessionId = sessionId,
                    consumerTag = consumerTag,
                    phase = "failed",
                    detail = detail
                )
                return persistStatus(
                    phase = "failed",
                    detail = detail,
                    tunFd = launchBundle.tunFd,
                    contextPath = launchBundle.contextPath,
                    backendConfigPath = launchBundle.backendConfigPath,
                    logPath = launchBundle.logPath
                )
            }

            val selection = AndroidNativeBackendRuntimeRegistry.current(context, launchBundle)
            val initialLaunchResult = AndroidNativeBackendLaunchResult(
                phase = "launching",
                detail = "Android native backend launch was dispatched to the background runtime worker.",
                runtimeName = selection.runtime.runtimeName,
                runtimeSelection = selection.selectionSummary
            )
            val launchState =
                if (launchBundle.backendHint == "android_native_proxy_fallback") {
                    initialLaunchResult.phase
                } else {
                    AndroidTunnelService.updateBackendHandoffSessionState(
                        sessionId = sessionId,
                        consumerTag = consumerTag,
                        phase = initialLaunchResult.phase,
                        detail = initialLaunchResult.detail
                    )
                }
            val payload = initialLaunchResult.toJson(
                launchBundlePath = launchBundlePath,
                statusPath = statusFile.absolutePath,
                bundle = launchBundle
            ).apply {
                put("launch_state", launchState)
            }
            runCatching {
                statusFile.writeText(payload.toString(2))
                val global = globalStatusFile(context)
                if (global.absolutePath != statusFile.absolutePath) {
                    global.writeText(payload.toString(2))
                }
            }

            Thread({
                val launchResult = selection.runtime.launch(context, launchBundle).copy(
                    runtimeSelection = selection.selectionSummary
                )

                val activeSession = AndroidTunnelService.getBackendHandoffSessionId()
                val sessionWasCleared =
                    launchBundle.backendHint == "android_native_handoff_required" &&
                        activeSession != sessionId
                if (sessionWasCleared) {
                    launchResult.runningHandle?.let { handle ->
                        runCatching { selection.runtime.stop(handle) }
                    }
                    val cancelledPayload = launchResult.copy(
                        phase = "cancelled",
                        detail = "Android native backend launch finished after the handoff session had already been cleared."
                    )
                    persistBackgroundStatus(
                        context = context,
                        statusFile = statusFile,
                        launchBundlePath = launchBundlePath,
                        launchBundle = launchBundle,
                        sessionId = sessionId,
                        consumerTag = consumerTag,
                        launchResult = cancelledPayload
                    )
                    return@Thread
                }

                launchResult.runningHandle?.let { handle ->
                    AndroidTunnelService.registerRunningBackend(handle)
                }
                persistBackgroundStatus(
                    context = context,
                    statusFile = statusFile,
                    launchBundlePath = launchBundlePath,
                    launchBundle = launchBundle,
                    sessionId = sessionId,
                    consumerTag = consumerTag,
                    launchResult = launchResult
                )
            }, "rkn-libbox-launch-$sessionId").start()

            payload.toString()
        } catch (error: Throwable) {
            val detail = error.message ?: error::class.java.simpleName
            if (sessionId.isNotBlank()) {
                AndroidTunnelService.updateBackendHandoffSessionState(
                    sessionId = sessionId,
                    consumerTag = consumerTag,
                    phase = "failed",
                    detail = detail
                )
            }
            persistStatus(
                phase = "failed",
                detail = "Android native backend seam crashed while preparing the launch bundle: $detail"
            )
        }
    }

    @JvmStatic
    fun getStatusPath(context: Context): String = globalStatusFile(context).absolutePath

    @JvmStatic
    fun abortClaimedSession(context: Context, sessionId: String, reason: String): String {
        val statusFile = resolveStatusFileForSession(context, sessionId)
        val stopState = AndroidTunnelService.abortBackendLaunch(sessionId, reason)
        val payload = JSONObject().apply {
            put("phase", "failed")
            put("detail", "Android native backend launch aborted: $reason")
            put("session_id", sessionId)
            put("consumer_tag", DEFAULT_CONSUMER_TAG)
            put("status_path", statusFile.absolutePath)
            put("launch_state", "failed")
            put("abort_state", stopState)
        }

        runCatching {
            statusFile.parentFile?.mkdirs()
            statusFile.writeText(payload.toString(2))
            val global = globalStatusFile(context)
            global.parentFile?.mkdirs()
            if (global.absolutePath != statusFile.absolutePath) {
                global.writeText(payload.toString(2))
            }
        }

        return stopState
    }

    @JvmStatic
    fun getStatusState(context: Context): String {
        val statusFile = globalStatusFile(context)
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

    private fun resolveStatusFileForSession(context: Context, sessionId: String): File {
        val global = globalStatusFile(context)
        val statusPath = runCatching {
            val payload = JSONObject(global.readText())
            val payloadSession = payload.optString("session_id")
            payload
                .optString("status_path")
                .takeIf {
                    it.isNotBlank() &&
                        (payloadSession.isBlank() || payloadSession == sessionId)
                }
        }.getOrNull()

        return statusPath?.let { File(it) } ?: global
    }

    private fun persistBackgroundStatus(
        context: Context,
        statusFile: File,
        launchBundlePath: String,
        launchBundle: AndroidNativeBackendLaunchBundlePayload,
        sessionId: String,
        consumerTag: String,
        launchResult: AndroidNativeBackendLaunchResult
    ) {
        val effectiveLaunchResult = mergeWithExistingStatusIfFurther(statusFile, launchResult)
        val launchState =
            if (launchBundle.backendHint == "android_native_proxy_fallback") {
                effectiveLaunchResult.phase
            } else {
                AndroidTunnelService.updateBackendHandoffSessionState(
                    sessionId = sessionId,
                    consumerTag = consumerTag,
                    phase = effectiveLaunchResult.phase,
                    detail = effectiveLaunchResult.detail
                )
            }
        val payload = effectiveLaunchResult.toJson(
            launchBundlePath = launchBundlePath,
            statusPath = statusFile.absolutePath,
            bundle = launchBundle
        ).apply {
            put("launch_state", launchState)
        }
        runCatching {
            statusFile.parentFile?.mkdirs()
            statusFile.writeText(payload.toString(2))
            val global = globalStatusFile(context)
            global.parentFile?.mkdirs()
            if (global.absolutePath != statusFile.absolutePath) {
                global.writeText(payload.toString(2))
            }
        }
    }

    private fun mergeWithExistingStatusIfFurther(
        statusFile: File,
        launchResult: AndroidNativeBackendLaunchResult
    ): AndroidNativeBackendLaunchResult {
        if (!launchResult.phase.startsWith("launching") &&
            !launchResult.phase.startsWith("starting")
        ) {
            return launchResult
        }

        val existing = runCatching {
            if (!statusFile.exists()) {
                return@runCatching null
            }
            JSONObject(statusFile.readText())
        }.getOrNull() ?: return launchResult

        val existingPhase = existing.optString("phase").ifBlank {
            existing.optString("launch_state", "")
        }
        if (existingPhase.isBlank()) {
            return launchResult
        }

        if (existingPhase.startsWith("launching") || existingPhase.startsWith("starting")) {
            return launchResult
        }

        return launchResult.copy(
            phase = existingPhase,
            detail = existing.optString("detail").ifBlank { launchResult.detail },
            runtimeName = existing.optString("runtime_name").ifBlank { launchResult.runtimeName },
            runtimeSelection = existing.optString("runtime_selection").ifBlank {
                launchResult.runtimeSelection
            },
            backendConfigSummary = existing.optString("backend_config_summary").ifBlank {
                launchResult.backendConfigSummary
            }
        )
    }
}
