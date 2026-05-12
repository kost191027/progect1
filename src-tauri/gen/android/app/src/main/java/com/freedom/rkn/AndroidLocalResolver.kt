package com.freedom.rkn

import android.net.DnsResolver
import android.os.CancellationSignal
import android.system.ErrnoException
import io.nekohasekai.libbox.ExchangeContext
import io.nekohasekai.libbox.Func
import io.nekohasekai.libbox.LocalDNSTransport
import java.net.InetAddress
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference

object AndroidLocalResolver : LocalDNSTransport {
    private const val DNS_TIMEOUT_SECONDS = 5L
    private val dnsExecutor = Executors.newFixedThreadPool(4) { runnable ->
        Thread(runnable, "rkn-android-local-dns").apply {
            isDaemon = true
        }
    }

    @Volatile
    private var appContextRef: android.content.Context? = null

    fun init(context: android.content.Context) {
        appContextRef = context.applicationContext
    }

    private fun appContext(): android.content.Context =
        appContextRef ?: error("AndroidLocalResolver is not initialized yet.")

    override fun raw(): Boolean = true

    override fun exchange(ctx: ExchangeContext, message: ByteArray) {
        val network = AndroidDefaultNetworkMonitor.currentNetwork(appContext())
            ?: error("missing default interface")
        val signal = CancellationSignal()
        ctx.onCancel(object : Func {
            override fun invoke() {
                signal.cancel()
            }
        })

        val failure = AtomicReference<Throwable?>(null)
        val latch = CountDownLatch(1)
        val callback = object : DnsResolver.Callback<ByteArray> {
            override fun onAnswer(answer: ByteArray, rcode: Int) {
                if (rcode == 0) {
                    ctx.rawSuccess(answer)
                } else {
                    ctx.errorCode(rcode)
                }
                latch.countDown()
            }

            override fun onError(error: DnsResolver.DnsException) {
                when (val cause = error.cause) {
                    is ErrnoException -> ctx.errnoCode(cause.errno)
                    else -> failure.set(error)
                }
                latch.countDown()
            }
        }

        DnsResolver.getInstance().rawQuery(
            network,
            message,
            DnsResolver.FLAG_NO_RETRY,
            dnsExecutor,
            signal,
            callback
        )

        if (!latch.await(DNS_TIMEOUT_SECONDS, TimeUnit.SECONDS)) {
            signal.cancel()
            ctx.errorCode(2)
            return
        }

        failure.get()?.let {
            ctx.errorCode(2)
        }
    }

    override fun lookup(ctx: ExchangeContext, network: String, domain: String) {
        val defaultNetwork = AndroidDefaultNetworkMonitor.currentNetwork(appContext())
            ?: error("missing default interface")
        val signal = CancellationSignal()
        ctx.onCancel(object : Func {
            override fun invoke() {
                signal.cancel()
            }
        })

        val failure = AtomicReference<Throwable?>(null)
        val latch = CountDownLatch(1)
        val callback = object : DnsResolver.Callback<Collection<InetAddress>> {
            override fun onAnswer(answer: Collection<InetAddress>, rcode: Int) {
                if (rcode == 0) {
                    ctx.success(answer.mapNotNull { it.hostAddress }.joinToString("\n"))
                } else {
                    ctx.errorCode(rcode)
                }
                latch.countDown()
            }

            override fun onError(error: DnsResolver.DnsException) {
                when (val cause = error.cause) {
                    is ErrnoException -> ctx.errnoCode(cause.errno)
                    else -> failure.set(error)
                }
                latch.countDown()
            }
        }

        val type = when {
            network.endsWith("4") -> DnsResolver.TYPE_A
            network.endsWith("6") -> DnsResolver.TYPE_AAAA
            else -> null
        }

        if (type != null) {
            DnsResolver.getInstance().query(
                defaultNetwork,
                domain,
                type,
                DnsResolver.FLAG_NO_RETRY,
                dnsExecutor,
                signal,
                callback
            )
        } else {
            DnsResolver.getInstance().query(
                defaultNetwork,
                domain,
                DnsResolver.FLAG_NO_RETRY,
                dnsExecutor,
                signal,
                callback
            )
        }

        if (!latch.await(DNS_TIMEOUT_SECONDS, TimeUnit.SECONDS)) {
            signal.cancel()
            ctx.errorCode(2)
            return
        }

        val error = failure.get()
        if (error != null) {
            ctx.errorCode(2)
        }
    }
}
