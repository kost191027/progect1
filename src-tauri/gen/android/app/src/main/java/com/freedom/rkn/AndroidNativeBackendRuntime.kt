package com.freedom.rkn

import org.json.JSONObject
import java.io.File

data class AndroidNativeBackendLaunchBundlePayload(
    val sessionId: String,
    val consumerTag: String,
    val backendHint: String,
    val tunFd: Int,
    val tunState: String,
    val tunAddress: String,
    val tunPrefixLength: Int,
    val tunRoute: String,
    val tunMtu: Int,
    val configPath: String,
    val backendConfigPath: String,
    val contextPath: String,
    val claimPath: String,
    val logPath: String,
    val sessionDir: String,
    val runtimeLogPath: String,
    val runtimeStatusPath: String,
    val tunFdOwnership: String,
    val protectApiAvailable: Boolean,
) {
    companion object {
        fun fromFile(path: String): AndroidNativeBackendLaunchBundlePayload {
            val file = File(path)
            val payload = JSONObject(file.readText())
            return AndroidNativeBackendLaunchBundlePayload(
                sessionId = payload.optString("session_id"),
                consumerTag = payload.optString("consumer_tag", "rkn_android_native_backend_seam"),
                backendHint = payload.optString("backend_hint"),
                tunFd = payload.optInt("tun_fd", -1),
                tunState = payload.optString("tun_state"),
                tunAddress = payload.optString("tun_address"),
                tunPrefixLength = payload.optInt("tun_prefix_length", -1),
                tunRoute = payload.optString("tun_route"),
                tunMtu = payload.optInt("tun_mtu", -1),
                configPath = payload.optString("config_path"),
                backendConfigPath = payload.optString("backend_config_path"),
                contextPath = payload.optString("context_path"),
                claimPath = payload.optString("claim_path"),
                logPath = payload.optString("log_path"),
                sessionDir = payload.optString("session_dir"),
                runtimeLogPath = payload.optString("runtime_log_path"),
                runtimeStatusPath = payload.optString("runtime_status_path"),
                tunFdOwnership = payload.optString("tun_fd_ownership"),
                protectApiAvailable = payload.optBoolean("protect_api_available", false),
            )
        }
    }
}

data class AndroidNativeBackendRunningHandle(
    val runtimeId: String,
    val sessionId: String,
    val consumerTag: String,
    val sessionDir: String,
    val runtimeLogPath: String,
    val runtimeStatusPath: String,
)

data class AndroidNativeBackendLaunchResult(
    val phase: String,
    val detail: String,
    val runtimeName: String,
    val backendConfigSummary: String = "",
    val runtimeSelection: String = "",
    val runningHandle: AndroidNativeBackendRunningHandle? = null,
) {
    fun toJson(
        launchBundlePath: String,
        statusPath: String,
        bundle: AndroidNativeBackendLaunchBundlePayload,
    ): JSONObject {
        return JSONObject().apply {
            put("phase", phase)
            put("detail", detail)
            put("runtime_name", runtimeName)
            put("backend_config_summary", backendConfigSummary)
            put("runtime_selection", runtimeSelection)
            put("session_id", bundle.sessionId)
            put("consumer_tag", bundle.consumerTag)
            put("launch_bundle_path", launchBundlePath)
            put("claim_path", bundle.claimPath)
            put("status_path", statusPath)
            put("tun_fd", bundle.tunFd)
            put("tun_state", bundle.tunState)
            put("context_path", bundle.contextPath)
            put("backend_config_path", bundle.backendConfigPath)
            put("log_path", bundle.logPath)
            put("session_dir", bundle.sessionDir)
            put("runtime_log_path", bundle.runtimeLogPath)
            put("runtime_status_path", bundle.runtimeStatusPath)
            put("tun_fd_ownership", bundle.tunFdOwnership)
        }
    }
}

data class AndroidNativeBackendAvailability(
    val available: Boolean,
    val detail: String,
)

data class AndroidNativeBackendRuntimeSelection(
    val runtime: AndroidNativeBackendRuntime,
    val preferredRuntime: String,
    val selectionSummary: String,
)

interface AndroidNativeBackendRuntime {
    val runtimeId: String
    val runtimeName: String

    fun availability(
        context: android.content.Context,
        bundle: AndroidNativeBackendLaunchBundlePayload,
    ): AndroidNativeBackendAvailability

    fun launch(
        context: android.content.Context,
        bundle: AndroidNativeBackendLaunchBundlePayload,
    ): AndroidNativeBackendLaunchResult

    fun stop(handle: AndroidNativeBackendRunningHandle): String
}
