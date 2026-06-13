package com.freedom.rkn

import android.content.Context
import android.content.Context.CONNECTIVITY_SERVICE
import android.net.ConnectivityManager
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
import java.io.File
import java.net.NetworkInterface as JNetworkInterface
import java.security.KeyStore
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import org.json.JSONArray
import org.json.JSONObject

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
        bundle: AndroidNativeBackendLaunchBundlePayload
    ): AndroidNativeBackendAvailability {
        if (!BuildConfig.ANDROID_NATIVE_BACKEND_RUNTIME.equals("libbox", ignoreCase = true)) {
            return AndroidNativeBackendAvailability(
                available = false,
                detail = "libbox runtime is not the selected Android native backend in this build."
            )
        }

        if (!BuildConfig.ANDROID_NATIVE_BACKEND_LIBBOX_AAR_PRESENT) {
            return AndroidNativeBackendAvailability(
                available = false,
                detail = "libbox runtime is selected, but app/libs/libbox.aar is not wired into this Android build (${BuildConfig.ANDROID_NATIVE_BACKEND_LIBBOX_AAR_PATH})."
            )
        }

        val nativeLibraryDir = context.applicationInfo.nativeLibraryDir ?: ""
        val probe = LibboxRuntimeProbe.inspect(context)
        val libboxCandidate = listOf(
            File(nativeLibraryDir, "libbox.so"),
            File(nativeLibraryDir, "libbox-jni.so")
        ).firstOrNull { it.exists() }

        if (libboxCandidate == null) {
            return AndroidNativeBackendAvailability(
                available = false,
                detail = "libbox runtime is selected, but no linked native backend library was found in nativeLibraryDir=$nativeLibraryDir. Probe: ${probe.summary()}"
            )
        }

        return AndroidNativeBackendAvailability(
            available = true,
            detail = "libbox candidate library detected at ${libboxCandidate.absolutePath}. Probe: ${probe.summary()}"
        )
    }

    override fun launch(
        context: Context,
        bundle: AndroidNativeBackendLaunchBundlePayload
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
                "backend summary: $backendSummary"
            )

            // 6A.4.1 DoD #3: before we touch libbox again, synchronously drain any
            // runtime state left over from a previous session. Without this, the
            // previous CommandServer may still be alive inside the Go runtime, and
            // setup / startOrReloadService below can deadlock on the
            // libbox JNI side, leaving the launch thread stuck forever.
            drainLingeringRuntimeStates(bundle.runtimeLogPath, bundle.sessionId)

            writeRuntimeLog(bundle.runtimeLogPath, "step: ensureLibboxSetup begin")
            ensureLibboxSetup(context, bundle)
            writeRuntimeLog(bundle.runtimeLogPath, "step: ensureLibboxSetup done")

            writeRuntimeLog(bundle.runtimeLogPath, "step: create platform interface")
            val tunOpened = AtomicBoolean(false)
            val platformInterface = RknLibboxPlatformInterface(
                context.applicationContext,
                bundle
            ) { detail ->
                if (tunOpened.compareAndSet(false, true)) {
                    persistRuntimePhase(
                        bundle = bundle,
                        phase = "ready",
                        detail = detail,
                        runtimeName = runtimeName,
                        runtimeSelection = "preferred=${BuildConfig.ANDROID_NATIVE_BACKEND_RUNTIME}, selected=$runtimeId",
                        backendConfigSummary = backendSummary
                    )
                }
            }
            writeRuntimeLog(bundle.runtimeLogPath, "step: create command server")
            val handler = RknLibboxCommandServerHandler(context.applicationContext, bundle)
            // This libbox AAR exposes the CommandServer-driven startup flow that SFA wraps
            // inside its own BoxService layer. There is no Libbox.newService()/BoxService
            // symbol in the current binary, so the native runtime must bootstrap through
            // CommandServer.startOrReloadService().
            val commandServer = Libbox.newCommandServer(handler, platformInterface)
            val configContent = configFile.readText()
            val usesTunInbound = configContainsTunInbound(configContent)
            val runningHandle = AndroidNativeBackendRunningHandle(
                runtimeId = runtimeId,
                sessionId = bundle.sessionId,
                consumerTag = bundle.consumerTag,
                sessionDir = bundle.sessionDir,
                runtimeLogPath = bundle.runtimeLogPath,
                runtimeStatusPath = bundle.runtimeStatusPath
            )
            val state = LibboxRuntimeState(
                sessionId = bundle.sessionId,
                commandServer = commandServer,
                platformInterface = platformInterface,
                runtimeLogPath = bundle.runtimeLogPath
            )

            // Register before startOrReloadService(): on Android this call can block
            // inside libbox while the VPN handoff is still settling. Stop must be
            // able to close the live CommandServer even during that launch window.
            synchronized(runtimeStates) {
                runtimeStates[bundle.sessionId] = state
            }
            AndroidTunnelService.registerRunningBackend(runningHandle)
            writeRuntimeLog(
                bundle.runtimeLogPath,
                "registered libbox runtime handle before CommandServer startup",
                "session=${bundle.sessionId}"
            )

            try {
                writeRuntimeLog(bundle.runtimeLogPath, "step: commandServer.start begin")
                commandServer.start()
                writeRuntimeLog(bundle.runtimeLogPath, "step: commandServer.start done")
                writeRuntimeLog(
                    bundle.runtimeLogPath,
                    "step: commandServer.startOrReloadService begin",
                    "config_size=${configContent.length}"
                )
                commandServer.startOrReloadService(configContent, OverrideOptions())
                writeRuntimeLog(
                    bundle.runtimeLogPath,
                    "step: commandServer.startOrReloadService done"
                )
            } catch (error: Throwable) {
                runCatching { state.close("launch-failure", 5_000) }
                synchronized(runtimeStates) {
                    runtimeStates.remove(bundle.sessionId)
                }
                AndroidTunnelService.clearRunningBackend(runningHandle)
                throw error
            }

            if (!usesTunInbound && tunOpened.compareAndSet(false, true)) {
                persistRuntimePhase(
                    bundle = bundle,
                    phase = "ready",
                    detail = "libbox proxy fallback runtime started without a tun inbound and is ready to serve the local mixed proxy.",
                    runtimeName = runtimeName,
                    runtimeSelection = "preferred=${BuildConfig.ANDROID_NATIVE_BACKEND_RUNTIME}, selected=$runtimeId",
                    backendConfigSummary = backendSummary
                )
            }

            writeRuntimeLog(
                bundle.runtimeLogPath,
                if (tunOpened.get()) "libbox runtime is ready" else "libbox runtime is starting and waiting for openTun()",
                "libbox version: ${runCatching { Libbox.version() }.getOrDefault("unknown")}",
                "launch config: ${bundle.configPath}"
            )

            AndroidNativeBackendLaunchResult(
                phase = if (tunOpened.get()) "ready" else "starting",
                detail = if (tunOpened.get()) {
                    if (usesTunInbound) {
                        "libbox runtime started and consumed the Android handoff session with a duplicated VpnService-owned TUN fd."
                    } else {
                        "libbox proxy fallback runtime started without a tun inbound and exposed the local mixed proxy."
                    }
                } else {
                    "libbox runtime started and is waiting for the first openTun callback before marking the tunnel ready."
                },
                runtimeName = runtimeName,
                backendConfigSummary = backendSummary,
                runningHandle = runningHandle
            )
        }.getOrElse { error ->
            writeRuntimeLog(
                bundle.runtimeLogPath,
                "libbox launch failed: ${error.message ?: error::class.java.simpleName}"
            )
            AndroidNativeBackendLaunchResult(
                phase = "failed",
                detail = "libbox runtime failed to start: ${error.message ?: error::class.java.simpleName}. Probe: $probeSummary",
                runtimeName = runtimeName,
                backendConfigSummary = backendSummary
            )
        }
    }

    override fun stop(handle: AndroidNativeBackendRunningHandle): String {
        val state = synchronized(runtimeStates) {
            runtimeStates[handle.sessionId]
        } ?: return "idle(runtime=${handle.runtimeId}, session=${handle.sessionId})"

        return state.close("service-stop", 15_000).also {
            synchronized(runtimeStates) {
                if (state.closed) {
                    runtimeStates.remove(handle.sessionId)
                }
            }
        }
    }

    private fun ensureLibboxSetup(
        context: Context,
        bundle: AndroidNativeBackendLaunchBundlePayload
    ) {
        val baseDir = File(context.filesDir, "libbox/base").apply { mkdirs() }
        val tempDir = File(context.cacheDir, "libbox/temp").apply { mkdirs() }
        val workingDir = File(context.filesDir, "libbox/work").apply { mkdirs() }
        val options = SetupOptions().apply {
            basePath = baseDir.absolutePath
            workingPath = workingDir.absolutePath
            tempPath = tempDir.absolutePath
            fixAndroidStack = true
            logMaxLines = 3_000
            debug = BuildConfig.DEBUG
        }

        // Platform helpers are process-scoped and do not need setupLock.
        writeRuntimeLog(bundle.runtimeLogPath, "step: AndroidLocalResolver.init begin")
        AndroidLocalResolver.init(context.applicationContext)
        writeRuntimeLog(bundle.runtimeLogPath, "step: AndroidLocalResolver.init done")

        writeRuntimeLog(
            bundle.runtimeLogPath,
            "step: AndroidDefaultNetworkMonitor.ensureStarted begin"
        )
        AndroidDefaultNetworkMonitor.ensureStarted(context.applicationContext)
        writeRuntimeLog(
            bundle.runtimeLogPath,
            "step: AndroidDefaultNetworkMonitor.ensureStarted done"
        )

        synchronized(setupLock) {
            if (setupInitialized) {
                writeRuntimeLog(
                    bundle.runtimeLogPath,
                    "step: Libbox.setup already initialized; reusing process-wide setup options",
                    "base=${baseDir.absolutePath}",
                    "working=${workingDir.absolutePath}",
                    "temp=${tempDir.absolutePath}"
                )
            } else {
                writeRuntimeLog(
                    bundle.runtimeLogPath,
                    "step: Libbox.setup begin",
                    "base=${baseDir.absolutePath}",
                    "working=${workingDir.absolutePath}",
                    "temp=${tempDir.absolutePath}"
                )
                Libbox.setup(options)
                setupInitialized = true
                writeRuntimeLog(bundle.runtimeLogPath, "step: Libbox.setup done")
            }
        }
    }

    /**
     * Synchronously close any runtime states left in the registry from previous sessions.
     *
     * Per 6A.4.1 DoD #3, Stop must drain both the VPN-service anchor and the local runtime
     * without hanging state. The stop path itself has a bounded wait for responsiveness, so
     * state can occasionally outlive a user-initiated Stop. Before we bootstrap a new libbox
     * session we must make sure no previous CommandServer is still alive inside the Go
     * runtime — otherwise `Libbox.setup` and `startOrReloadService` can deadlock
     * and the launch thread is stuck forever, which is exactly the
     * "stayed in a pending launch state" failure the Rust side reports.
     */
    private fun drainLingeringRuntimeStates(runtimeLogPath: String, newSessionId: String) {
        val lingering = synchronized(runtimeStates) {
            if (runtimeStates.isEmpty()) {
                return
            }
            runtimeStates.values.toList()
        }

        writeRuntimeLog(
            runtimeLogPath,
            "draining ${lingering.size} lingering libbox runtime state(s) before new session $newSessionId"
        )
        for (state in lingering) {
            runCatching {
                val detail = state.close("new-session-$newSessionId", 12_000)
                writeRuntimeLog(
                    runtimeLogPath,
                    "drained previous session ${state.sessionId}: $detail"
                )
                if (state.closed) {
                    synchronized(runtimeStates) {
                        runtimeStates.remove(state.sessionId)
                    }
                } else {
                    throw IllegalStateException(
                        "Previous Android VPN runtime is still stopping. Wait a few seconds and start again."
                    )
                }
            }.onFailure { error ->
                writeRuntimeLog(
                    runtimeLogPath,
                    "drain of previous session ${state.sessionId} reported error: ${error.message ?: error::class.java.simpleName}"
                )
                throw error
            }
        }
    }

    private fun markSessionStopping(sessionId: String, reason: String): Boolean {
        synchronized(runtimeStates) {
            val state = runtimeStates[sessionId] ?: return false
            if (state.stopping) {
                return false
            }
            state.stopping = true
            writeRuntimeLog(
                state.runtimeLogPath,
                "libbox runtime requested service stop",
                "reason=$reason"
            )
            return true
        }
    }

    private fun readBackendConfigSummary(path: String): String = runCatching {
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

    private fun persistRuntimePhase(
        bundle: AndroidNativeBackendLaunchBundlePayload,
        phase: String,
        detail: String,
        runtimeName: String,
        runtimeSelection: String,
        backendConfigSummary: String
    ) {
        val launchState =
            if (bundle.backendHint == "android_native_proxy_fallback") {
                phase
            } else {
                AndroidTunnelService.updateBackendHandoffSessionState(
                    sessionId = bundle.sessionId,
                    consumerTag = bundle.consumerTag,
                    phase = phase,
                    detail = detail
                )
            }
        val payload = JSONObject().apply {
            put("phase", phase)
            put("detail", detail)
            put("runtime_name", runtimeName)
            put("backend_config_summary", backendConfigSummary)
            put("runtime_selection", runtimeSelection)
            put("session_id", bundle.sessionId)
            put("consumer_tag", bundle.consumerTag)
            put("launch_bundle_path", bundle.claimPath)
            put("claim_path", bundle.claimPath)
            put("status_path", bundle.runtimeStatusPath)
            put("tun_fd", bundle.tunFd)
            put("tun_state", bundle.tunState)
            put("context_path", bundle.contextPath)
            put("backend_config_path", bundle.backendConfigPath)
            put("log_path", bundle.logPath)
            put("session_dir", bundle.sessionDir)
            put("runtime_log_path", bundle.runtimeLogPath)
            put("runtime_status_path", bundle.runtimeStatusPath)
            put("tun_fd_ownership", bundle.tunFdOwnership)
            put("launch_state", launchState)
        }

        runCatching {
            val statusFile = File(bundle.runtimeStatusPath)
            statusFile.parentFile?.mkdirs()
            statusFile.writeText(payload.toString(2))

            val global = File(bundle.logPath).parentFile?.let { logDir ->
                File(logDir.parentFile ?: logDir, "android_native_backend_status.json")
            }
            if (global != null && global.absolutePath != statusFile.absolutePath) {
                global.writeText(payload.toString(2))
            }
        }
    }

    private data class LibboxRuntimeState(
        val sessionId: String,
        val commandServer: CommandServer,
        val platformInterface: RknLibboxPlatformInterface,
        val runtimeLogPath: String,
        var stopping: Boolean = false
    ) {
        private val closeStarted = AtomicBoolean(false)
        private val closeFinished = CountDownLatch(1)
        private val closeDetails = mutableListOf<String>()
        private val closeDetailsLock = Any()

        @Volatile
        var closed: Boolean = false

        fun close(reason: String, timeoutMs: Long = 5_000): String {
            stopping = true
            if (closeStarted.compareAndSet(false, true)) {
                Thread({
                    try {
                        runCatching {
                            commandServer.closeService()
                            appendCloseDetail("closeService=ok")
                        }.onFailure { error ->
                            appendCloseDetail(
                                "closeService=${error.message ?: error::class.java.simpleName}"
                            )
                        }
                        runCatching {
                            commandServer.close()
                            appendCloseDetail("close=ok")
                        }.onFailure { error ->
                            appendCloseDetail(
                                "close=${error.message ?: error::class.java.simpleName}"
                            )
                        }
                    } finally {
                        platformInterface.close()
                        closed = true
                        closeFinished.countDown()
                    }
                }, "rkn-libbox-stop-$sessionId").start()
            }

            // 6A.4.1 DoD #3: do not silently forget a still-closing libbox runtime.
            // Quick Stop -> Start can otherwise collide with a live CommandServer and
            // leave Android stuck in the foreground "working" notification state.
            val settled = closeFinished.await(timeoutMs, TimeUnit.MILLISECONDS)
            val finalDetail = if (settled) {
                snapshotCloseDetails().joinToString(",")
            } else {
                (snapshotCloseDetails() + "close=background").joinToString(",")
            }

            File(runtimeLogPath).appendText(
                "libbox runtime stop requested for session $sessionId, reason=$reason, settled=$settled, detail=$finalDetail\n"
            )
            return if (settled) {
                "stopped(runtime=libbox, session=$sessionId, reason=$reason, detail=$finalDetail)"
            } else {
                "stopping(runtime=libbox, session=$sessionId, reason=$reason, detail=$finalDetail)"
            }
        }

        private fun appendCloseDetail(detail: String) {
            synchronized(closeDetailsLock) {
                closeDetails += detail
            }
        }

        private fun snapshotCloseDetails(): List<String> = synchronized(closeDetailsLock) {
            closeDetails.toList()
        }
    }

    private class RknLibboxCommandServerHandler(
        private val appContext: Context,
        private val bundle: AndroidNativeBackendLaunchBundlePayload
    ) : CommandServerHandler {
        override fun getSystemProxyStatus(): SystemProxyStatus = SystemProxyStatus().apply {
            available = false
            enabled = false
        }

        override fun serviceReload() {
            writeRuntimeLog(
                bundle.runtimeLogPath,
                "libbox requested service reload, but the current backend keeps reload delegated to the app-level restart flow."
            )
        }

        override fun serviceStop() {
            if (markSessionStopping(bundle.sessionId, "native-service-stop")) {
                writeRuntimeLog(
                    bundle.runtimeLogPath,
                    "libbox requested serviceStop(); marking runtime stopped and letting the app-level monitor drain the VPN service anchor"
                )
                persistRuntimePhase(
                    bundle = bundle,
                    phase = "stopped",
                    detail = "libbox requested serviceStop()",
                    runtimeName = runtimeName,
                    runtimeSelection = "",
                    backendConfigSummary = ""
                )
            }
        }

        override fun setSystemProxyEnabled(isEnabled: Boolean) {
            writeRuntimeLog(
                bundle.runtimeLogPath,
                "libbox requested system proxy state change",
                "enabled=$isEnabled"
            )
        }

        override fun writeDebugMessage(message: String) {
            writeRuntimeLog(bundle.runtimeLogPath, "[libbox-debug] $message")
            if (BuildConfig.DEBUG) {
                Log.d(TAG, message)
            }
        }
    }

    private class RknLibboxPlatformInterface(
        private val appContext: Context,
        private val bundle: AndroidNativeBackendLaunchBundlePayload,
        private val onTunReady: (String) -> Unit
    ) : PlatformInterface {
        private val tunLock = Any()

        @Volatile
        private var duplicatedTunDescriptor: ParcelFileDescriptor? = null

        override fun usePlatformAutoDetectInterfaceControl(): Boolean = true

        override fun autoDetectInterfaceControl(fd: Int) {
            val protected = AndroidVpnBridge.protectSocketFd(appContext, fd)
            writeRuntimeLog(
                bundle.runtimeLogPath,
                "protect(fd=$fd) => $protected"
            )
            if (!protected && bundle.protectApiAvailable) {
                throw IllegalStateException(
                    "Failed to protect outbound socket fd=$fd from the Android VPN loop."
                )
            }
        }

        override fun openTun(options: TunOptions): Int {
            synchronized(tunLock) {
                duplicatedTunDescriptor?.let { descriptor ->
                    runCatching { descriptor.close() }
                }

                val duplicated = AndroidTunnelService.openTunnelInterface(options)
                    ?: throw IllegalStateException(
                        "AndroidTunnelService could not establish and duplicate the active VpnService TUN interface for libbox."
                    )
                duplicatedTunDescriptor = duplicated
                val dnsServer =
                    runCatching { options.dnsServerAddress.value }.getOrDefault("unavailable")
                writeRuntimeLog(
                    bundle.runtimeLogPath,
                    "openTun() established and duplicated the VpnService-owned TUN fd",
                    "handoff_trace_fd=${bundle.tunFd}",
                    "dup_fd=${duplicated.fd}",
                    "ownership=${bundle.tunFdOwnership}",
                    "options: mtu=${runCatching {
                        options.mtu
                    }.getOrDefault(-1)}, autoRoute=${runCatching {
                        options.autoRoute
                    }.getOrDefault(false)}, strictRoute=${runCatching {
                        options.strictRoute
                    }.getOrDefault(false)}, dns=$dnsServer"
                )
                onTunReady(
                    "libbox runtime opened the Android TUN interface and consumed the VpnService-owned handoff session."
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
            destinationPort: Int
        ): ConnectionOwner = ConnectionOwner().apply {
            userId = -1
            userName = ""
            processPath = ""
            setAndroidPackageNames(SimpleStringIterator(emptyList()))
        }

        override fun startDefaultInterfaceMonitor(
            listener: io.nekohasekai.libbox.InterfaceUpdateListener
        ) {
            writeRuntimeLog(
                bundle.runtimeLogPath,
                "libbox requested default interface monitor; publishing the current non-VPN Android network."
            )
            AndroidDefaultNetworkMonitor.setListener(appContext, listener)
        }

        override fun closeDefaultInterfaceMonitor(
            listener: io.nekohasekai.libbox.InterfaceUpdateListener
        ) {
            AndroidDefaultNetworkMonitor.setListener(appContext, null)
        }

        override fun getInterfaces(): NetworkInterfaceIterator {
            val interfaces = mutableListOf<NetworkInterface>()
            val connectivity =
                appContext.getSystemService(CONNECTIVITY_SERVICE) as ConnectivityManager
            val defaultNetwork = AndroidDefaultNetworkMonitor.currentNetwork(appContext)
            val enumeration = JNetworkInterface.getNetworkInterfaces()
            while (enumeration != null && enumeration.hasMoreElements()) {
                val current = enumeration.nextElement()
                val addresses = current.interfaceAddresses
                    ?.mapNotNull { address ->
                        val host = address.address?.hostAddress ?: return@mapNotNull null
                        "$host/${address.networkPrefixLength}"
                    }
                    .orEmpty()

                if (!AndroidDefaultNetworkMonitor.isUsableInterfaceName(current.name) ||
                    addresses.isEmpty()
                ) {
                    continue
                }

                val iface = NetworkInterface().apply {
                    index = runCatching { current.index }.getOrDefault(0)
                    name = current.name ?: ""
                    mtu = runCatching { current.mtu }.getOrDefault(0)
                    setAddresses(SimpleStringIterator(addresses))
                    flags = AndroidDefaultNetworkMonitor.interfaceFlags(appContext, name)
                    type = AndroidDefaultNetworkMonitor.interfaceType(appContext, defaultNetwork)
                    setDNSServer(
                        SimpleStringIterator(
                            AndroidDefaultNetworkMonitor.interfaceDnsServers(appContext, name)
                        )
                    )
                    val capabilities =
                        defaultNetwork?.let { connectivity.getNetworkCapabilities(it) }
                    metered =
                        capabilities?.hasCapability(
                            android.net.NetworkCapabilities.NET_CAPABILITY_NOT_METERED
                        ) ==
                        false
                }
                interfaces += iface
            }
            return SimpleNetworkInterfaceIterator(interfaces)
        }

        override fun underNetworkExtension(): Boolean = false

        override fun includeAllNetworks(): Boolean = true

        override fun clearDNSCache() {
        }

        override fun readWIFIState(): WIFIState? = null

        override fun localDNSTransport(): LocalDNSTransport? = AndroidLocalResolver

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
                        append(
                            android.util.Base64.encodeToString(
                                cert.encoded,
                                android.util.Base64.NO_WRAP
                            )
                        )
                        append("\n-----END CERTIFICATE-----")
                    }
                }
            }
            return SimpleStringIterator(certificates)
        }

        override fun sendNotification(notification: Notification) {
            writeRuntimeLog(
                bundle.runtimeLogPath,
                "libbox notification: id=${notification.identifier}, title=${notification.title}, body=${notification.body}"
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

    private class SimpleStringIterator(private val values: List<String>) : StringIterator {
        private var index = 0

        override fun hasNext(): Boolean = index < values.size

        override fun len(): Int = values.size

        override fun next(): String = values[index++]
    }

    private class SimpleNetworkInterfaceIterator(private val values: List<NetworkInterface>) :
        NetworkInterfaceIterator {
        private var index = 0

        override fun hasNext(): Boolean = index < values.size

        override fun next(): NetworkInterface = values[index++]
    }
}
