package com.freedom.rkn

import android.content.Context
import android.os.Build
import android.os.ParcelFileDescriptor
import android.util.Log
import io.nekohasekai.libbox.CommandServer
import io.nekohasekai.libbox.CommandServerHandler
import io.nekohasekai.libbox.ConnectionOwner
import io.nekohasekai.libbox.Libbox
import io.nekohasekai.libbox.LocalDNSTransport
import io.nekohasekai.libbox.NetworkInterface
import io.nekohasekai.libbox.NetworkInterfaceIterator
import io.nekohasekai.libbox.Notification
import io.nekohasekai.libbox.OverrideOptions
import io.nekohasekai.libbox.PlatformInterface
import io.nekohasekai.libbox.SetupOptions
import io.nekohasekai.libbox.StringIterator
import io.nekohasekai.libbox.SystemProxyStatus
import io.nekohasekai.libbox.TunOptions
import io.nekohasekai.libbox.WIFIState
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.net.NetworkInterface as JNetworkInterface
import java.security.KeyStore

object LibboxAndroidNativeBackendRuntime : AndroidNativeBackendRuntime {
    override val runtimeId: String = "libbox"
    override val runtimeName: String = "libbox_android_native_backend"
    private const val TAG = "LibboxRuntime"
    private val setupLock = Any()
    private val runtimeStates = mutableMapOf<String, LibboxRuntimeState>()
    @Volatile
    private var setupInitialized = false

    override fun availability(
        context: Context,
        bundle: AndroidNativeBackendLaunchBundlePayload,
    ): AndroidNativeBackendAvailability {
        if (!BuildConfig.ANDROID_NATIVE_BACKEND_RUNTIME.equals("libbox", ignoreCase = true)) {
            return AndroidNativeBackendAvailability(
                available = false,
                detail = "libbox runtime is not the selected Android native backend in this build.",
            )
        }

        if (!BuildConfig.ANDROID_NATIVE_BACKEND_LIBBOX_AAR_PRESENT) {
            return AndroidNativeBackendAvailability(
                available = false,
                detail = "libbox runtime is selected, but app/libs/libbox.aar is not wired into this Android build (${BuildConfig.ANDROID_NATIVE_BACKEND_LIBBOX_AAR_PATH}).",
            )
        }

        val nativeLibraryDir = context.applicationInfo.nativeLibraryDir ?: ""
        val probe = LibboxRuntimeProbe.inspect(context)
        val libboxCandidate = listOf(
            File(nativeLibraryDir, "libbox.so"),
            File(nativeLibraryDir, "libbox-jni.so"),
        ).firstOrNull { it.exists() }

        if (libboxCandidate == null) {
            return AndroidNativeBackendAvailability(
                available = false,
                detail = "libbox runtime is selected, but no linked native backend library was found in nativeLibraryDir=$nativeLibraryDir. Probe: ${probe.summary()}",
            )
        }

        return AndroidNativeBackendAvailability(
            available = true,
            detail = "libbox candidate library detected at ${libboxCandidate.absolutePath}. Probe: ${probe.summary()}",
        )
    }

    override fun launch(
        context: Context,
        bundle: AndroidNativeBackendLaunchBundlePayload,
    ): AndroidNativeBackendLaunchResult {
        val probeSummary = LibboxRuntimeProbe.inspect(context).summary()
        val backendSummary = readBackendConfigSummary(bundle.backendConfigPath)

        return runCatching {
            // configPath is the full Android runtime config with tun inbound intact.
            // backendConfigPath remains the stripped diagnostic/handoff payload for other adapters.
            val configFile = File(bundle.configPath)
            require(configFile.exists()) {
                "libbox runtime could not find the Android runtime config at ${bundle.configPath}."
            }

            File(bundle.sessionDir).mkdirs()
            writeRuntimeLog(
                bundle.runtimeLogPath,
                "libbox runtime selected for session ${bundle.sessionId}",
                "tun fd ownership: ${bundle.tunFdOwnership}",
                "probe: $probeSummary",
                "backend summary: $backendSummary",
            )

            ensureLibboxSetup(context, bundle)

            val existing = synchronized(runtimeStates) {
                runtimeStates.remove(bundle.sessionId)
            }
            existing?.close("replace-existing-session")

            val platformInterface = RknLibboxPlatformInterface(context.applicationContext, bundle)
            val handler = RknLibboxCommandServerHandler(context.applicationContext, bundle)
            val commandServer = Libbox.newCommandServer(handler, platformInterface)
            val configContent = configFile.readText()
            require(configContainsTunInbound(configContent)) {
                "libbox runtime expected configPath=${bundle.configPath} to contain a tun inbound, but it did not. backendConfigPath=${bundle.backendConfigPath} stays diagnostic-only and cannot replace the full runtime config."
            }

            try {
                commandServer.start()
                commandServer.startOrReloadService(configContent, OverrideOptions())
            } catch (error: Throwable) {
                runCatching { commandServer.close() }
                platformInterface.close()
                throw error
            }

            val state = LibboxRuntimeState(
                sessionId = bundle.sessionId,
                commandServer = commandServer,
                platformInterface = platformInterface,
                runtimeLogPath = bundle.runtimeLogPath,
            )
            synchronized(runtimeStates) {
                runtimeStates[bundle.sessionId] = state
            }

            writeRuntimeLog(
                bundle.runtimeLogPath,
                "libbox runtime is ready",
                "libbox version: ${runCatching { Libbox.version() }.getOrDefault("unknown")}",
                "launch config: ${bundle.configPath}",
            )

            AndroidNativeBackendLaunchResult(
                phase = "ready",
                detail = "libbox runtime started and consumed the Android handoff session with a duplicated VpnService-owned TUN fd.",
                runtimeName = runtimeName,
                backendConfigSummary = backendSummary,
                runningHandle = AndroidNativeBackendRunningHandle(
                    runtimeId = runtimeId,
                    sessionId = bundle.sessionId,
                    consumerTag = bundle.consumerTag,
                    sessionDir = bundle.sessionDir,
                    runtimeLogPath = bundle.runtimeLogPath,
                    runtimeStatusPath = bundle.runtimeStatusPath,
                ),
            )
        }.getOrElse { error ->
            writeRuntimeLog(
                bundle.runtimeLogPath,
                "libbox launch failed: ${error.message ?: error::class.java.simpleName}",
            )
            AndroidNativeBackendLaunchResult(
                phase = "failed",
                detail = "libbox runtime failed to start: ${error.message ?: error::class.java.simpleName}. Probe: $probeSummary",
                runtimeName = runtimeName,
                backendConfigSummary = backendSummary,
            )
        }
    }

    override fun stop(handle: AndroidNativeBackendRunningHandle): String {
        val state = synchronized(runtimeStates) {
            runtimeStates[handle.sessionId]
        } ?: return "idle(runtime=${handle.runtimeId}, session=${handle.sessionId})"

        return state.close("service-stop").also {
            synchronized(runtimeStates) {
                runtimeStates.remove(handle.sessionId)
            }
        }
    }

    private fun ensureLibboxSetup(
        context: Context,
        bundle: AndroidNativeBackendLaunchBundlePayload,
    ) {
        val baseDir = File(context.filesDir, "libbox/base").apply { mkdirs() }
        val tempDir = File(context.cacheDir, "libbox/temp").apply { mkdirs() }
        val workingDir = File(bundle.sessionDir, "libbox-work").apply { mkdirs() }
        val options = SetupOptions().apply {
            basePath = baseDir.absolutePath
            workingPath = workingDir.absolutePath
            tempPath = tempDir.absolutePath
            fixAndroidStack = true
            logMaxLines = 3_000
            debug = BuildConfig.DEBUG
            crashReportSource = TAG
        }

        synchronized(setupLock) {
            if (setupInitialized) {
                Libbox.reloadSetupOptions(options)
                writeRuntimeLog(
                    bundle.runtimeLogPath,
                    "libbox setup options reloaded",
                    "base=${baseDir.absolutePath}",
                    "working=${workingDir.absolutePath}",
                    "temp=${tempDir.absolutePath}",
                )
            } else {
                Libbox.setup(options)
                setupInitialized = true
                writeRuntimeLog(
                    bundle.runtimeLogPath,
                    "libbox global setup complete",
                    "base=${baseDir.absolutePath}",
                    "working=${workingDir.absolutePath}",
                    "temp=${tempDir.absolutePath}",
                )
            }
        }
    }

    private fun markSessionStopping(
        sessionId: String,
        reason: String,
    ): Boolean {
        synchronized(runtimeStates) {
            val state = runtimeStates[sessionId] ?: return false
            if (state.stopping) {
                return false
            }
            state.stopping = true
            writeRuntimeLog(
                state.runtimeLogPath,
                "libbox runtime requested service stop",
                "reason=$reason",
            )
            return true
        }
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

    private fun configContainsTunInbound(raw: String): Boolean {
        return runCatching {
            val payload = JSONObject(raw)
            val inbounds = payload.optJSONArray("inbounds") ?: return false
            for (index in 0 until inbounds.length()) {
                val type = inbounds.optJSONObject(index)?.optString("type")
                if (type == "tun") {
                    return true
                }
            }
            false
        }.getOrDefault(false)
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

        return if (tags.isEmpty()) "untagged" else tags.joinToString(",")
    }

    private fun writeRuntimeLog(path: String, vararg lines: String) {
        runCatching {
            val file = File(path)
            file.parentFile?.mkdirs()
            file.appendText(lines.joinToString(separator = "\n", postfix = "\n"))
        }
    }

    private data class LibboxRuntimeState(
        val sessionId: String,
        val commandServer: CommandServer,
        val platformInterface: RknLibboxPlatformInterface,
        val runtimeLogPath: String,
        var stopping: Boolean = false,
    ) {
        fun close(reason: String): String {
            stopping = true
            val details = mutableListOf<String>()
            runCatching {
                commandServer.closeService()
                details += "closeService=ok"
            }.onFailure { error ->
                details += "closeService=${error.message ?: error::class.java.simpleName}"
            }
            runCatching {
                commandServer.close()
                details += "close=ok"
            }.onFailure { error ->
                details += "close=${error.message ?: error::class.java.simpleName}"
            }
            platformInterface.close()
            File(runtimeLogPath).appendText(
                "libbox runtime stopped for session $sessionId, reason=$reason, detail=${details.joinToString(",")}\n",
            )
            return "stopped(runtime=libbox, session=$sessionId, reason=$reason, detail=${details.joinToString(",")})"
        }
    }

    private class RknLibboxCommandServerHandler(
        private val appContext: Context,
        private val bundle: AndroidNativeBackendLaunchBundlePayload,
    ) : CommandServerHandler {
        override fun getSystemProxyStatus(): SystemProxyStatus {
            return SystemProxyStatus().apply {
                available = false
                enabled = false
            }
        }

        override fun serviceReload() {
            writeRuntimeLog(
                bundle.runtimeLogPath,
                "libbox requested service reload, but the current backend keeps reload delegated to the app-level restart flow.",
            )
        }

        override fun serviceStop() {
            if (markSessionStopping(bundle.sessionId, "native-service-stop")) {
                writeRuntimeLog(
                    bundle.runtimeLogPath,
                    "forwarding native serviceStop() into AndroidTunnelService stop intent",
                )
                AndroidVpnBridge.stopTunnelService(appContext)
            }
        }

        override fun setSystemProxyEnabled(isEnabled: Boolean) {
            writeRuntimeLog(
                bundle.runtimeLogPath,
                "libbox requested system proxy state change",
                "enabled=$isEnabled",
            )
        }

        override fun triggerNativeCrash() {
            writeRuntimeLog(
                bundle.runtimeLogPath,
                "libbox requested triggerNativeCrash(), ignored in RKN runtime.",
            )
        }

        override fun writeDebugMessage(message: String) {
            writeRuntimeLog(bundle.runtimeLogPath, "[libbox-debug] $message")
            Log.d(TAG, message)
        }
    }

    private class RknLibboxPlatformInterface(
        private val appContext: Context,
        private val bundle: AndroidNativeBackendLaunchBundlePayload,
    ) : PlatformInterface {
        private val tunLock = Any()
        @Volatile
        private var duplicatedTunDescriptor: ParcelFileDescriptor? = null

        override fun usePlatformAutoDetectInterfaceControl(): Boolean = true

        override fun autoDetectInterfaceControl(fd: Int) {
            val protected = AndroidVpnBridge.protectSocketFd(appContext, fd)
            writeRuntimeLog(
                bundle.runtimeLogPath,
                "protect(fd=$fd) => $protected",
            )
            if (!protected && bundle.protectApiAvailable) {
                throw IllegalStateException("Failed to protect outbound socket fd=$fd from the Android VPN loop.")
            }
        }

        override fun openTun(options: TunOptions): Int {
            synchronized(tunLock) {
                duplicatedTunDescriptor?.let { descriptor ->
                    runCatching { descriptor.close() }
                }

                val duplicated = AndroidTunnelService.duplicateTunnelInterface()
                    ?: throw IllegalStateException(
                        "AndroidTunnelService could not duplicate the active VpnService TUN interface for libbox.",
                    )
                duplicatedTunDescriptor = duplicated
                writeRuntimeLog(
                    bundle.runtimeLogPath,
                    "openTun() duplicated the VpnService-owned TUN fd",
                    "handoff_trace_fd=${bundle.tunFd}",
                    "dup_fd=${duplicated.fd}",
                    "ownership=${bundle.tunFdOwnership}",
                    "options: mtu=${runCatching { options.mtu }.getOrDefault(-1)}, autoRoute=${runCatching { options.autoRoute }.getOrDefault(false)}, strictRoute=${runCatching { options.strictRoute }.getOrDefault(false)}",
                )
                return duplicated.fd
            }
        }

        override fun useProcFS(): Boolean = Build.VERSION.SDK_INT < Build.VERSION_CODES.Q

        override fun findConnectionOwner(
            ipProtocol: Int,
            sourceAddress: String,
            sourcePort: Int,
            destinationAddress: String,
            destinationPort: Int,
        ): ConnectionOwner {
            return ConnectionOwner().apply {
                userId = -1
                userName = ""
                processPath = ""
                setAndroidPackageNames(SimpleStringIterator(emptyList()))
            }
        }

        override fun startDefaultInterfaceMonitor(listener: io.nekohasekai.libbox.InterfaceUpdateListener) {
            writeRuntimeLog(
                bundle.runtimeLogPath,
                "libbox requested default interface monitor; current RKN runtime keeps this as a no-op first iteration.",
            )
        }

        override fun closeDefaultInterfaceMonitor(listener: io.nekohasekai.libbox.InterfaceUpdateListener) {
        }

        override fun getInterfaces(): NetworkInterfaceIterator {
            val interfaces = mutableListOf<NetworkInterface>()
            val enumeration = JNetworkInterface.getNetworkInterfaces()
            while (enumeration != null && enumeration.hasMoreElements()) {
                val current = enumeration.nextElement()
                val addresses = current.interfaceAddresses
                    ?.mapNotNull { address ->
                        val host = address.address?.hostAddress ?: return@mapNotNull null
                        "$host/${address.networkPrefixLength}"
                    }
                    .orEmpty()

                val iface = NetworkInterface().apply {
                    index = current.index
                    name = current.name ?: ""
                    mtu = runCatching { current.mtu }.getOrDefault(0)
                    setAddresses(SimpleStringIterator(addresses))
                    flags = 0
                    type = Libbox.InterfaceTypeOther
                    setDNSServer(SimpleStringIterator(emptyList()))
                    metered = false
                }
                interfaces += iface
            }
            return SimpleNetworkInterfaceIterator(interfaces)
        }

        override fun underNetworkExtension(): Boolean = false

        override fun includeAllNetworks(): Boolean = false

        override fun clearDNSCache() {
        }

        override fun readWIFIState(): WIFIState? = null

        override fun localDNSTransport(): LocalDNSTransport? = null

        override fun systemCertificates(): StringIterator {
            val certificates = mutableListOf<String>()
            runCatching {
                val keyStore = KeyStore.getInstance("AndroidCAStore")
                keyStore.load(null, null)
                val aliases = keyStore.aliases()
                while (aliases.hasMoreElements()) {
                    val alias = aliases.nextElement()
                    val cert = keyStore.getCertificate(alias) ?: continue
                    certificates += buildString {
                        append("-----BEGIN CERTIFICATE-----\n")
                        append(android.util.Base64.encodeToString(cert.encoded, android.util.Base64.NO_WRAP))
                        append("\n-----END CERTIFICATE-----")
                    }
                }
            }
            return SimpleStringIterator(certificates)
        }

        override fun startNeighborMonitor(listener: io.nekohasekai.libbox.NeighborUpdateListener) {
            writeRuntimeLog(
                bundle.runtimeLogPath,
                "libbox requested neighbor monitor; current RKN runtime keeps this as a no-op first iteration.",
            )
        }

        override fun closeNeighborMonitor(listener: io.nekohasekai.libbox.NeighborUpdateListener) {
        }

        override fun registerMyInterface(name: String) {
            writeRuntimeLog(bundle.runtimeLogPath, "registerMyInterface($name)")
        }

        override fun sendNotification(notification: Notification) {
            writeRuntimeLog(
                bundle.runtimeLogPath,
                "libbox notification: id=${notification.identifier}, title=${notification.title}, body=${notification.body}",
            )
        }

        fun close() {
            synchronized(tunLock) {
                duplicatedTunDescriptor?.let { descriptor ->
                    runCatching { descriptor.close() }
                }
                duplicatedTunDescriptor = null
            }
        }
    }

    private class SimpleStringIterator(
        private val values: List<String>,
    ) : StringIterator {
        private var index = 0

        override fun hasNext(): Boolean = index < values.size

        override fun len(): Int = values.size

        override fun next(): String = values[index++]
    }

    private class SimpleNetworkInterfaceIterator(
        private val values: List<NetworkInterface>,
    ) : NetworkInterfaceIterator {
        private var index = 0

        override fun hasNext(): Boolean = index < values.size

        override fun next(): NetworkInterface = values[index++]
    }
}
