import { existsSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const args = process.argv.slice(2);
const cwd = process.cwd();
const androidRoot = join(cwd, "src-tauri", "gen", "android");
const gradleExecutable = process.platform === "win32" ? "gradlew.bat" : "gradlew";
const gradlePath = join(androidRoot, gradleExecutable);

const fallbackJavaHomes = [
  "/Library/Java/JavaVirtualMachines/temurin-17.jdk/Contents/Home",
  "/Library/Java/JavaVirtualMachines/temurin-21.jdk/Contents/Home",
  "/usr/local/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home",
  "/usr/local/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home",
];

const fallbackAndroidHome = existsSync("/usr/local/share/android-commandlinetools")
  ? "/usr/local/share/android-commandlinetools"
  : "";

const fallbackNdkHomes = [
  process.env.NDK_HOME,
  process.env.ANDROID_NDK_HOME,
  existsSync("/usr/local/share/android-ndk/source.properties") ? "/usr/local/share/android-ndk" : "",
  existsSync(join(fallbackAndroidHome, "ndk", "29.0.14206865", "source.properties"))
    ? join(fallbackAndroidHome, "ndk", "29.0.14206865")
    : "",
].filter(Boolean);

function fail(message) {
  console.error(message);
  process.exit(1);
}

function firstExistingDir(candidates) {
  return candidates.find((candidate) => candidate && existsSync(candidate)) || "";
}

function isAndroidCompatibleJavaHome(javaHome) {
  return Boolean(javaHome) && !javaHome.includes("temurin-26");
}

function resolvePrebuiltBin(ndkHome) {
  const prebuiltRoot = join(ndkHome, "toolchains", "llvm", "prebuilt");
  if (!existsSync(prebuiltRoot)) {
    return "";
  }

  const hostDir =
    readdirSync(prebuiltRoot, { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .map((entry) => join(prebuiltRoot, entry.name))
      .find((dir) => existsSync(join(dir, "bin", "llvm-ranlib"))) || "";

  return hostDir ? join(hostDir, "bin") : "";
}

if (!existsSync(gradlePath)) {
  fail(`Android Gradle wrapper was not found at ${gradlePath}.`);
}

const javaHome = isAndroidCompatibleJavaHome(process.env.JAVA_HOME || "")
  ? process.env.JAVA_HOME
  : firstExistingDir(fallbackJavaHomes);
const androidHome = process.env.ANDROID_HOME || process.env.ANDROID_SDK_ROOT || fallbackAndroidHome;
const ndkHome = firstExistingDir(fallbackNdkHomes);

if (!javaHome || !existsSync(javaHome)) {
  fail("Android Gradle launcher could not resolve JAVA_HOME.");
}

const llvmBin = ndkHome ? resolvePrebuiltBin(ndkHome) : "";
const env = {
  ...process.env,
  JAVA_HOME: javaHome,
  ANDROID_HOME: androidHome || process.env.ANDROID_HOME || "",
  ANDROID_SDK_ROOT: androidHome || process.env.ANDROID_SDK_ROOT || "",
  NDK_HOME: ndkHome || process.env.NDK_HOME || "",
  ANDROID_NDK_HOME: ndkHome || process.env.ANDROID_NDK_HOME || "",
  PATH: [
    join(javaHome, "bin"),
    androidHome ? join(androidHome, "cmdline-tools", "latest", "bin") : "",
    androidHome ? join(androidHome, "platform-tools") : "",
    llvmBin,
    process.env.PATH || "",
  ].filter(Boolean).join(":"),
};

const gradleArgs = args.length > 0 ? args : [":app:androidKotlinQuality"];
const result = spawnSync(gradlePath, ["--project-dir", androidRoot, ...gradleArgs], {
  cwd,
  env,
  stdio: "inherit",
});

if (result.error) {
  fail(`Failed to launch Android Gradle command: ${result.error.message}`);
}

process.exit(result.status ?? 1);
