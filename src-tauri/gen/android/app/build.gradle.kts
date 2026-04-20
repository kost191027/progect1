import java.util.Properties
import java.nio.file.Files
import java.nio.file.StandardCopyOption

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

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

tasks.register("prepareAndroidSingboxSidecar") {
    inputs.file(androidBundledSingboxSource)
    outputs.file(androidBundledSingboxTarget)

    doLast {
        if (!androidBundledSingboxSource.exists()) {
            throw GradleException(
                "Android sing-box sidecar is missing at ${androidBundledSingboxSource.absolutePath}",
            )
        }

        androidBundledSingboxTarget.parentFile.mkdirs()
        Files.copy(
            androidBundledSingboxSource.toPath(),
            androidBundledSingboxTarget.toPath(),
            StandardCopyOption.REPLACE_EXISTING,
        )
        androidBundledSingboxTarget.setReadable(true, false)
        androidBundledSingboxTarget.setExecutable(true, false)
    }
}

tasks.named("preBuild").configure {
    dependsOn("prepareAndroidSingboxSidecar")
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = "tauri.build.gradle.kts")
