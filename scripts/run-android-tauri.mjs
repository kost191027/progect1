import { existsSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const args = process.argv.slice(2);

if (args.length === 0) {
  console.error("Usage: node scripts/run-android-tauri.mjs <tauri android args...>");
  process.exit(1);
}

const cwd = process.cwd();
const androidDevConfig = join(cwd, "src-tauri", "tauri.android-dev.conf.json");

const fallbackJavaHomes = [
  "/Library/Java/JavaVirtualMachines/temurin-17.jdk/Contents/Home",
  "/Library/Java/JavaVirtualMachines/temurin-21.jdk/Contents/Home",
  "/usr/local/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home",
  "/usr/local/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home",
  "/Library/Java/JavaVirtualMachines/temurin-26.jdk/Contents/Home",
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
  if (!javaHome) {
    return false;
  }

  return !javaHome.includes("temurin-26");
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

const fallbackJavaHome = firstExistingDir(fallbackJavaHomes);
const javaHome =
  isAndroidCompatibleJavaHome(process.env.JAVA_HOME || "")
    ? process.env.JAVA_HOME
    : fallbackJavaHome;
const androidHome = process.env.ANDROID_HOME || process.env.ANDROID_SDK_ROOT || fallbackAndroidHome;
const ndkHome = firstExistingDir(fallbackNdkHomes);

if (!javaHome || !existsSync(javaHome)) {
  fail("Android launcher could not resolve JAVA_HOME.");
}

if (!androidHome || !existsSync(androidHome)) {
  fail("Android launcher could not resolve ANDROID_HOME.");
}

if (!ndkHome || !existsSync(join(ndkHome, "source.properties"))) {
  fail("Android launcher could not resolve NDK_HOME.");
}

const llvmBin = resolvePrebuiltBin(ndkHome);
if (!llvmBin) {
  fail(`Android launcher could not resolve LLVM tools inside ${ndkHome}.`);
}

const targetClang = join(llvmBin, "aarch64-linux-android29-clang");
const targetAr = join(llvmBin, "llvm-ar");
const targetRanlib = join(llvmBin, "llvm-ranlib");

for (const tool of [targetClang, targetAr, targetRanlib]) {
  if (!existsSync(tool)) {
    fail(`Android launcher missing expected NDK tool: ${tool}`);
  }
}

const env = {
  ...process.env,
  JAVA_HOME: javaHome,
  ANDROID_HOME: androidHome,
  ANDROID_SDK_ROOT: androidHome,
  NDK_HOME: ndkHome,
  ANDROID_NDK_HOME: ndkHome,
  TARGET_CC: process.env.TARGET_CC || targetClang,
  TARGET_AR: process.env.TARGET_AR || targetAr,
  TARGET_RANLIB: process.env.TARGET_RANLIB || targetRanlib,
  CC_aarch64_linux_android: process.env.CC_aarch64_linux_android || targetClang,
  AR_aarch64_linux_android: process.env.AR_aarch64_linux_android || targetAr,
  RANLIB_aarch64_linux_android: process.env.RANLIB_aarch64_linux_android || targetRanlib,
  CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER:
    process.env.CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER || targetClang,
  CARGO_TARGET_AARCH64_LINUX_ANDROID_AR:
    process.env.CARGO_TARGET_AARCH64_LINUX_ANDROID_AR || targetAr,
  PATH: [
    join(javaHome, "bin"),
    join(androidHome, "cmdline-tools", "latest", "bin"),
    join(androidHome, "platform-tools"),
    llvmBin,
    process.env.PATH || "",
  ].join(":"),
};

const androidArgs = [...args];
const passthroughIndex = androidArgs.indexOf("--");
const configArgs = ["--config", androidDevConfig];

if (passthroughIndex === -1) {
  androidArgs.push(...configArgs);
} else {
  androidArgs.splice(passthroughIndex, 0, ...configArgs);
}

const result = spawnSync("tauri", ["android", ...androidArgs], {
  cwd,
  env,
  stdio: "inherit",
});

if (result.error) {
  fail(`Failed to launch tauri android command: ${result.error.message}`);
}

process.exit(result.status ?? 1);
