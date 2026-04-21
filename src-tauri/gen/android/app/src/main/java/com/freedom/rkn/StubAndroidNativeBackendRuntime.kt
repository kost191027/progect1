package com.freedom.rkn

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

object StubAndroidNativeBackendRuntime : AndroidNativeBackendRuntime {
    override val runtimeId: String = "stub"
    override val runtimeName: String = "stub_android_native_backend"

    override fun availability(
        context: Context,
        bundle: AndroidNativeBackendLaunchBundlePayload,
    ): AndroidNativeBackendAvailability {
        return AndroidNativeBackendAvailability(
            available = true,
            detail = "stub runtime is bundled as the fallback Android-native backend seam.",
        )
    }

    override fun launch(
        context: Context,
        bundle: AndroidNativeBackendLaunchBundlePayload,
    ): AndroidNativeBackendLaunchResult {
        val summary = readBackendConfigSummary(bundle.backendConfigPath)
        runCatching {
            File(bundle.runtimeLogPath).writeText(
                "stub runtime consumed session ${bundle.sessionId}\n" +
                    "tun fd ownership: ${bundle.tunFdOwnership}\n" +
                    "backend summary: $summary\n",
            )
        }
        val detail =
            "Android native backend stub validated the launch bundle and parsed the backend config, but no libbox/SFA-style runtime is linked yet. protect(fd) is " +
                if (bundle.protectApiAvailable) "available." else "unavailable."

        return AndroidNativeBackendLaunchResult(
            phase = "failed",
            detail = detail,
            runtimeName = runtimeName,
            backendConfigSummary = summary,
        )
    }

    override fun stop(handle: AndroidNativeBackendRunningHandle): String {
        return "idle(runtime=${handle.runtimeId}, session=${handle.sessionId})"
    }

    private fun readBackendConfigSummary(path: String): String {
        return runCatching {
            val payload = JSONObject(File(path).readText())
            val outbounds = payload.optJSONArray("outbounds")
            val dns = payload.optJSONObject("dns")
            val route = payload.optJSONObject("route")
            val outboundTags = outbounds.toTagList()
            val dnsServers = dns?.optJSONArray("servers")?.length() ?: 0
            val routeRules = route?.optJSONArray("rules")?.length() ?: 0
            "outbounds=${outbounds?.length() ?: 0}[$outboundTags], dns_servers=$dnsServers, route_rules=$routeRules"
        }.getOrElse { error ->
            "unavailable(${error.message ?: error::class.java.simpleName})"
        }
    }

    private fun JSONArray?.toTagList(): String {
        if (this == null || length() == 0) {
            return "none"
        }

        val tags = buildList {
            for (index in 0 until length()) {
                val tag = optJSONObject(index)?.optString("tag")?.takeIf { it.isNotBlank() }
                if (tag != null) {
                    add(tag)
                }
            }
        }

        return if (tags.isEmpty()) {
            "untagged"
        } else {
            tags.joinToString(",")
        }
    }
}
