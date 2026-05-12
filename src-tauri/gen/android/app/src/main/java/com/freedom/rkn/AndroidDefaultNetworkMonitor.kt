package com.freedom.rkn

import android.content.Context
import android.net.ConnectivityManager
import android.net.LinkProperties
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.system.OsConstants
import io.nekohasekai.libbox.InterfaceUpdateListener
import java.net.NetworkInterface

object AndroidDefaultNetworkMonitor {
    @Volatile
    private var defaultNetwork: Network? = null

    @Volatile
    private var listener: InterfaceUpdateListener? = null

    @Volatile
    private var callbackRegistered = false
    private var networkCallback: ConnectivityManager.NetworkCallback? = null
    private val monitorLock = Any()

    fun ensureStarted(context: Context) {
        synchronized(monitorLock) {
            val connectivity =
                context.applicationContext.getSystemService(
                    Context.CONNECTIVITY_SERVICE
                ) as ConnectivityManager
            defaultNetwork = resolveUnderlyingNetwork(connectivity)

            if (callbackRegistered) {
                notifyListener(context.applicationContext, defaultNetwork)
                return
            }

            val callback = object : ConnectivityManager.NetworkCallback() {
                override fun onAvailable(network: Network) {
                    defaultNetwork = resolveUnderlyingNetwork(connectivity)
                    notifyListener(context.applicationContext, defaultNetwork)
                }

                override fun onLost(network: Network) {
                    if (defaultNetwork == network || defaultNetwork == null) {
                        defaultNetwork = resolveUnderlyingNetwork(connectivity)
                    }
                    notifyListener(context.applicationContext, defaultNetwork)
                }

                override fun onLinkPropertiesChanged(
                    network: Network,
                    linkProperties: LinkProperties
                ) {
                    if (defaultNetwork == network ||
                        defaultNetwork == null ||
                        !isUsableInterface(linkProperties.interfaceName)
                    ) {
                        defaultNetwork = resolveUnderlyingNetwork(connectivity)
                        notifyListener(context.applicationContext, defaultNetwork)
                    }
                }
            }

            val request = NetworkRequest.Builder()
                .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
                .addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN)
                .build()
            connectivity.registerNetworkCallback(request, callback)
            networkCallback = callback
            callbackRegistered = true
            notifyListener(context.applicationContext, defaultNetwork)
        }
    }

    fun currentNetwork(context: Context): Network? {
        ensureStarted(context)
        val connectivity =
            context.applicationContext.getSystemService(
                Context.CONNECTIVITY_SERVICE
            ) as ConnectivityManager
        val current = defaultNetwork ?: resolveUnderlyingNetwork(connectivity)
        defaultNetwork = current
        return current
    }

    fun setListener(context: Context, newListener: InterfaceUpdateListener?) {
        listener = newListener
        if (newListener == null) {
            return
        }

        Thread({
            ensureStarted(context)
            notifyListener(context.applicationContext, defaultNetwork)
        }, "rkn-default-network-monitor").start()
    }

    private fun notifyListener(context: Context, network: Network?) {
        val currentListener = listener ?: return
        if (network == null) {
            currentListener.updateDefaultInterface("", -1, false, false)
            return
        }

        val connectivity =
            context.applicationContext.getSystemService(
                Context.CONNECTIVITY_SERVICE
            ) as ConnectivityManager
        val linkProperties = connectivity.getLinkProperties(network) ?: return
        val interfaceName = linkProperties.interfaceName ?: return
        if (!isUsableInterface(interfaceName)) {
            currentListener.updateDefaultInterface("", -1, false, false)
            return
        }
        val interfaceIndex = runCatching {
            NetworkInterface.getByName(interfaceName)?.index ?: -1
        }.getOrDefault(-1)
        currentListener.updateDefaultInterface(interfaceName, interfaceIndex, false, false)
    }

    fun interfaceFlags(context: Context, interfaceName: String): Int {
        val connectivity =
            context.applicationContext.getSystemService(
                Context.CONNECTIVITY_SERVICE
            ) as ConnectivityManager
        val network = currentNetwork(context)
        val capabilities = network?.let { connectivity.getNetworkCapabilities(it) }
        val networkInterface = runCatching { NetworkInterface.getByName(interfaceName) }.getOrNull()
        var flags = 0

        if (capabilities?.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) == true) {
            flags = OsConstants.IFF_UP or OsConstants.IFF_RUNNING
        }
        if (networkInterface?.isLoopback == true) {
            flags = flags or OsConstants.IFF_LOOPBACK
        }
        if (networkInterface?.isPointToPoint == true) {
            flags = flags or OsConstants.IFF_POINTOPOINT
        }
        if (runCatching { networkInterface?.supportsMulticast() == true }.getOrDefault(false)) {
            flags = flags or OsConstants.IFF_MULTICAST
        }

        return flags
    }

    fun interfaceType(context: Context, network: Network?): Int {
        val connectivity =
            context.applicationContext.getSystemService(
                Context.CONNECTIVITY_SERVICE
            ) as ConnectivityManager
        val capabilities = network?.let { connectivity.getNetworkCapabilities(it) }
        return when {
            capabilities?.hasTransport(
                NetworkCapabilities.TRANSPORT_WIFI
            ) == true -> io.nekohasekai.libbox.Libbox.InterfaceTypeWIFI
            capabilities?.hasTransport(
                NetworkCapabilities.TRANSPORT_CELLULAR
            ) == true -> io.nekohasekai.libbox.Libbox.InterfaceTypeCellular
            capabilities?.hasTransport(
                NetworkCapabilities.TRANSPORT_ETHERNET
            ) == true -> io.nekohasekai.libbox.Libbox.InterfaceTypeEthernet
            else -> io.nekohasekai.libbox.Libbox.InterfaceTypeOther
        }
    }

    fun interfaceDnsServers(context: Context, interfaceName: String): List<String> {
        val connectivity =
            context.applicationContext.getSystemService(
                Context.CONNECTIVITY_SERVICE
            ) as ConnectivityManager
        val network = currentNetwork(context) ?: return emptyList()
        val linkProperties = connectivity.getLinkProperties(network) ?: return emptyList()
        if (linkProperties.interfaceName != interfaceName) {
            return emptyList()
        }
        return linkProperties.dnsServers.mapNotNull { it.hostAddress }
    }

    fun isUsableInterfaceName(interfaceName: String?): Boolean = isUsableInterface(interfaceName)

    private fun resolveUnderlyingNetwork(connectivity: ConnectivityManager): Network? {
        val active = connectivity.activeNetwork
        if (active != null && isUsableNetwork(connectivity, active)) {
            return active
        }

        return connectivity.allNetworks.firstOrNull { candidate ->
            isUsableNetwork(connectivity, candidate)
        }
    }

    private fun isUsableNetwork(connectivity: ConnectivityManager, network: Network): Boolean {
        val capabilities = connectivity.getNetworkCapabilities(network) ?: return false
        if (!capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)) {
            return false
        }
        if (!capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN)) {
            return false
        }

        val interfaceName = connectivity.getLinkProperties(network)?.interfaceName
        return isUsableInterface(interfaceName)
    }

    private fun isUsableInterface(interfaceName: String?): Boolean {
        if (interfaceName.isNullOrBlank()) {
            return false
        }

        val normalized = interfaceName.lowercase()
        return !normalized.startsWith("tun") && !normalized.startsWith("rkn")
    }
}
