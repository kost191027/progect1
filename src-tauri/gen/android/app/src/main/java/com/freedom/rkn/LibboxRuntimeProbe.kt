package com.freedom.rkn

import android.content.Context
import dalvik.system.DexFile
import java.io.File

data class LibboxRuntimeProbeResult(
    val nativeLibraries: List<String>,
    val candidateClasses: List<String>
) {
    fun summary(): String {
        val nativeSummary = if (nativeLibraries.isEmpty()) {
            "native=none"
        } else {
            "native=${nativeLibraries.joinToString(",")}"
        }
        val classSummary = if (candidateClasses.isEmpty()) {
            "classes=none"
        } else {
            "classes=${candidateClasses.take(8).joinToString(",")}"
        }
        return "$nativeSummary; $classSummary"
    }
}

object LibboxRuntimeProbe {
    private val classHints =
        listOf(
            "io.nekohasekai",
            "libbox",
            "sagernet"
        )

    fun inspect(context: Context): LibboxRuntimeProbeResult {
        val nativeLibraries = inspectNativeLibraries(context)
        val candidateClasses = inspectCandidateClasses(context)
        return LibboxRuntimeProbeResult(
            nativeLibraries = nativeLibraries,
            candidateClasses = candidateClasses
        )
    }

    private fun inspectNativeLibraries(context: Context): List<String> {
        val nativeLibraryDir = context.applicationInfo.nativeLibraryDir ?: return emptyList()
        val dir = File(nativeLibraryDir)
        if (!dir.exists() || !dir.isDirectory) {
            return emptyList()
        }

        return dir.listFiles()
            ?.map { it.name }
            ?.filter { name ->
                name.contains("libbox", ignoreCase = true) ||
                    name.contains("box", ignoreCase = true)
            }
            ?.sorted()
            ?: emptyList()
    }

    private fun inspectCandidateClasses(context: Context): List<String> = runCatching {
        val dexFile = DexFile(context.packageCodePath)
        dexFile.entries().asSequence()
            .filter { entry ->
                classHints.any { hint -> entry.contains(hint, ignoreCase = true) }
            }
            .take(20)
            .toList()
    }.getOrElse {
        emptyList()
    }
}
