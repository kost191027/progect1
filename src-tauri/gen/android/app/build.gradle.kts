import io.gitlab.arturbosch.detekt.Detekt
import io.gitlab.arturbosch.detekt.extensions.DetektExtension
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.util.Properties
import org.jlleitschuh.gradle.ktlint.KtlintExtension
import org.jlleitschuh.gradle.ktlint.reporter.ReporterType

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
    id("io.gitlab.arturbosch.detekt") version "1.23.8"
    id("org.jlleitschuh.gradle.ktlint") version "12.1.1"
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

val androidLibboxAar = file("libs/libbox.aar")
val hasLocalLibboxAar = androidLibboxAar.exists()

android {
    compileSdk = 36
    namespace = "com.freedom.rkn"
    packaging {
        jniLibs.useLegacyPackaging = true
    }
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "com.freedom.rkn"
        minSdk = 29
        targetSdk = 36
        ndk {
            abiFilters += listOf("arm64-v8a")
        }
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
        buildConfigField("String", "ANDROID_NATIVE_BACKEND_RUNTIME", "\"libbox\"")
        buildConfigField(
            "boolean",
            "ANDROID_NATIVE_BACKEND_LIBBOX_AAR_PRESENT",
            hasLocalLibboxAar.toString()
        )
        buildConfigField(
            "String",
            "ANDROID_NATIVE_BACKEND_LIBBOX_AAR_PATH",
            "\"${androidLibboxAar.absolutePath}\""
        )
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {
                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
            }
        }
        getByName("release") {
            isMinifyEnabled = true
            proguardFiles(
                *fileTree(".") { include("**/*.pro") }
                    .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                    .toList().toTypedArray()
            )
        }
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
    buildFeatures {
        buildConfig = true
    }
}

rust {
    rootDirRel = "../../../"
}

val androidBundledSingboxSource = file("../../../bins/sing-box-aarch64-linux-android")
val androidBundledSingboxTarget =
    file("src/main/jniLibs/arm64-v8a/libsingbox.so")

tasks.register("removeICloudDuplicateAndroidArtifacts") {
    doLast {
        val iCloudDuplicatePattern = Regex(""".* \d+\..*""")
        listOf(
            file("src/main/java"),
            file("src/main/assets"),
            file("build")
        ).forEach { root ->
            if (root.exists()) {
                root.walkTopDown()
                    .filter { candidate ->
                        candidate.isFile &&
                            candidate.name.matches(iCloudDuplicatePattern)
                    }
                    .forEach { duplicate ->
                        duplicate.delete()
                    }
            }
        }
    }
}

tasks.register("prepareAndroidSingboxSidecar") {
    inputs.file(androidBundledSingboxSource)
    outputs.file(androidBundledSingboxTarget)

    doLast {
        if (!androidBundledSingboxSource.exists()) {
            throw GradleException(
                "Android sing-box sidecar is missing at ${androidBundledSingboxSource.absolutePath}"
            )
        }

        androidBundledSingboxTarget.parentFile.mkdirs()
        Files.copy(
            androidBundledSingboxSource.toPath(),
            androidBundledSingboxTarget.toPath(),
            StandardCopyOption.REPLACE_EXISTING
        )
        androidBundledSingboxTarget.setReadable(true, false)
        androidBundledSingboxTarget.setExecutable(true, false)
    }
}

tasks.named("preBuild").configure {
    dependsOn("removeICloudDuplicateAndroidArtifacts")
    dependsOn("prepareAndroidSingboxSidecar")
}

configure<DetektExtension> {
    buildUponDefaultConfig = true
    allRules = false
    config.setFrom(rootProject.files("config/detekt.yml"))
    source.setFrom(files("src/main/java/com/freedom/rkn"))
    basePath = rootProject.projectDir.absolutePath
}

tasks.withType<Detekt>().configureEach {
    jvmTarget = "1.8"
    exclude("**/generated/**")
    reports {
        html.required.set(true)
        xml.required.set(true)
        txt.required.set(false)
        sarif.required.set(false)
    }
}

configure<KtlintExtension> {
    version.set("1.3.1")
    android.set(true)
    outputToConsole.set(true)
    ignoreFailures.set(false)
    reporters {
        reporter(ReporterType.CHECKSTYLE)
    }
    filter {
        exclude("**/generated/**")
        exclude("**/build/**")
        exclude("**/tauri.build.gradle.kts")
    }
}

tasks.register("androidKotlinQuality") {
    group = "verification"
    description = "Runs Kotlin static analysis and formatting gates for the Android layer."
    dependsOn("ktlintCheck")
    dependsOn("detekt")
}

dependencies {
    if (hasLocalLibboxAar) {
        implementation(files(androidLibboxAar))
    }
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = "tauri.build.gradle.kts")
