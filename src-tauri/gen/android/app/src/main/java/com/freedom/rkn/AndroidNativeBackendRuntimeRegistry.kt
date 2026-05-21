package com.freedom.rkn

object AndroidNativeBackendRuntimeRegistry {
    private val candidates =
        listOf(
            LibboxAndroidNativeBackendRuntime,
            StubAndroidNativeBackendRuntime
        )

    fun current(
        context: android.content.Context,
        bundle: AndroidNativeBackendLaunchBundlePayload
    ): AndroidNativeBackendRuntimeSelection {
        val preferredRuntime = BuildConfig.ANDROID_NATIVE_BACKEND_RUNTIME
        val preferredCandidate = candidates.firstOrNull { it.runtimeId == preferredRuntime }

        if (preferredCandidate != null) {
            val availability = preferredCandidate.availability(context, bundle)
            if (availability.available) {
                return AndroidNativeBackendRuntimeSelection(
                    runtime = preferredCandidate,
                    preferredRuntime = preferredRuntime,
                    selectionSummary = "preferred=$preferredRuntime, selected=${preferredCandidate.runtimeId}, detail=${availability.detail}"
                )
            }
        }

        val fallbackCandidate = candidates.firstOrNull { runtime ->
            runtime.availability(context, bundle).available
        } ?: StubAndroidNativeBackendRuntime
        val fallbackAvailability = fallbackCandidate.availability(context, bundle)
        val unavailablePreferred = preferredCandidate
            ?.availability(context, bundle)
            ?.detail
            ?.takeIf { preferredCandidate.runtimeId != fallbackCandidate.runtimeId }

        val selectionSummary = buildString {
            append(
                "preferred=$preferredRuntime, selected=${fallbackCandidate.runtimeId}, detail=${fallbackAvailability.detail}"
            )
            if (unavailablePreferred != null) {
                append(", preferred_unavailable=$unavailablePreferred")
            }
        }

        return AndroidNativeBackendRuntimeSelection(
            runtime = fallbackCandidate,
            preferredRuntime = preferredRuntime,
            selectionSummary = selectionSummary
        )
    }

    fun byId(runtimeId: String): AndroidNativeBackendRuntime? = candidates.firstOrNull {
        it.runtimeId == runtimeId
    }
}
