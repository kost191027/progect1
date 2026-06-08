import { existsSync, readdirSync, statSync } from "node:fs";
import { join, sep } from "node:path";
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

function optionConsumesValue(option) {
  return new Set([
    "--features",
    "-f",
    "--config",
    "-c",
    "--additional-watch-folders",
    "--host",
    "--port",
    "--root-certificate-path",
  ]).has(option);
}

function hasExplicitAndroidDevice(commandArgs) {
  let consumeNext = false;
  for (const arg of commandArgs.slice(1)) {
    if (arg === "--") {
      return false;
    }
    if (consumeNext) {
      consumeNext = false;
      continue;
    }
    if (optionConsumesValue(arg)) {
      consumeNext = true;
      continue;
    }
    if (arg.startsWith("-")) {
      continue;
    }
    return true;
  }
  return false;
}

function resolveAdbPath(androidHome) {
  const sdkAdb = join(androidHome, "platform-tools", "adb");
  return existsSync(sdkAdb) ? sdkAdb : "adb";
}

function parseAdbDevices(output) {
  return output
    .split(/\r?\n/)
    .slice(1)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const [serial = "", state = ""] = line.split(/\s+/, 2);
      return { serial, state, line };
    })
    .filter((device) => device.serial);
}

function detectAndroidDeviceForRun(adbPath) {
  const startResult = spawnSync(adbPath, ["start-server"], {
    encoding: "utf8",
  });

  if (startResult.status !== 0) {
    const message = [startResult.stderr, startResult.stdout].filter(Boolean).join("\n").trim();
    fail(
      [
        "Android launcher could not start adb server.",
        message,
        "",
        "Try:",
        "  adb kill-server",
        "  adb start-server",
        "  adb devices -l",
      ]
        .filter(Boolean)
        .join("\n"),
    );
  }

  const devicesResult = spawnSync(adbPath, ["devices", "-l"], {
    encoding: "utf8",
  });

  if (devicesResult.status !== 0) {
    const message = [devicesResult.stderr, devicesResult.stdout].filter(Boolean).join("\n").trim();
    fail(
      [
        "Android launcher could not list adb devices.",
        message,
        "",
        "Try:",
        "  adb kill-server",
        "  adb start-server",
        "  adb devices -l",
      ]
        .filter(Boolean)
        .join("\n"),
    );
  }

  const devices = parseAdbDevices(devicesResult.stdout);
  const readyDevices = devices.filter((device) => device.state === "device");

  if (readyDevices.length === 1) {
    return readyDevices[0].serial;
  }

  if (readyDevices.length > 1) {
    fail(
      [
        "Multiple Android devices are connected. Pass the device serial explicitly.",
        "",
        devicesResult.stdout.trim(),
        "",
        "Example:",
        `  npm run tauri:android:run -- ${readyDevices[0].serial}`,
      ].join("\n"),
    );
  }

  const unauthorized = devices.filter((device) => device.state === "unauthorized");
  if (unauthorized.length > 0) {
    fail(
      [
        "Android device is connected, but USB debugging is not authorized.",
        "",
        devicesResult.stdout.trim(),
        "",
        "Unlock the phone, accept the USB debugging fingerprint prompt, then run:",
        "  adb devices -l",
        "  npm run tauri:android:run",
      ].join("\n"),
    );
  }

  const offline = devices.filter((device) => device.state === "offline");
  if (offline.length > 0) {
    fail(
      [
        "Android device is visible to adb but currently offline.",
        "",
        devicesResult.stdout.trim(),
        "",
        "Try reconnecting USB, then run:",
        "  adb kill-server",
        "  adb start-server",
        "  adb devices -l",
      ].join("\n"),
    );
  }

  fail(
    [
      "No Android device in adb state 'device'. Tauri would fall back to emulator detection and fail.",
      "",
      devicesResult.stdout.trim(),
      "",
      "Check on the phone:",
      "  USB debugging is enabled",
      "  the USB debugging fingerprint prompt is accepted",
      "  USB mode is file transfer / media transfer",
      "",
      "Then run:",
      "  adb devices -l",
      "  npm run tauri:android:run",
    ].join("\n"),
  );
}

function collectDebugApks(dir) {
  if (!existsSync(dir)) {
    return [];
  }

  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const entryPath = join(dir, entry.name);
    if (entry.isDirectory()) {
      return collectDebugApks(entryPath);
    }
    if (entry.isFile() && entry.name.endsWith(".apk") && entryPath.includes(`${sep}debug${sep}`)) {
      return [entryPath];
    }
    return [];
  });
}

function androidDebugApkPath() {
  const apkRoot = join(
    cwd,
    "src-tauri",
    "gen",
    "android",
    "app",
    "build",
    "outputs",
    "apk",
  );
  const debugApks = collectDebugApks(apkRoot)
    .map((apkPath) => ({ apkPath, mtimeMs: statSync(apkPath).mtimeMs }))
    .sort((left, right) => right.mtimeMs - left.mtimeMs);

  if (debugApks.length > 0) {
    return debugApks[0].apkPath;
  }

  return join(apkRoot, "arm64", "debug", "app-arm64-debug.apk");
}

function printInstalledAndroidPackageInfo(adbPath, deviceSerial) {
  const packageResult = spawnSync(
    adbPath,
    ["-s", deviceSerial, "shell", "dumpsys", "package", "com.freedom.rkn"],
    {
      encoding: "utf8",
    },
  );

  if (packageResult.status !== 0) {
    return;
  }

  const packageInfo = packageResult.stdout
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) =>
      ["versionCode=", "versionName=", "firstInstallTime=", "lastUpdateTime="].some((prefix) =>
        line.startsWith(prefix),
      ),
    );

  if (packageInfo.length > 0) {
    console.log(["Installed Android package:", ...packageInfo.map((line) => `  ${line}`)].join("\n"));
  }
}

function installAndLaunchAndroidDebugApk(adbPath, deviceSerial) {
  const apkPath = androidDebugApkPath();
  if (!existsSync(apkPath)) {
    fail(`Android debug APK was not produced at ${apkPath}.`);
  }

  console.log(`Installing Android debug APK: ${apkPath}`);

  spawnSync(adbPath, ["-s", deviceSerial, "shell", "am", "force-stop", "com.freedom.rkn"], {
    stdio: "inherit",
  });

  const installResult = spawnSync(adbPath, ["-s", deviceSerial, "install", "-r", apkPath], {
    stdio: "inherit",
  });
  if (installResult.status !== 0) {
    process.exit(installResult.status ?? 1);
  }

  printInstalledAndroidPackageInfo(adbPath, deviceSerial);

  spawnSync(adbPath, ["-s", deviceSerial, "shell", "am", "force-stop", "com.freedom.rkn"], {
    stdio: "inherit",
  });

  const launchResult = spawnSync(
    adbPath,
    ["-s", deviceSerial, "shell", "am", "start", "-n", "com.freedom.rkn/.MainActivity"],
    { stdio: "inherit" },
  );
  process.exit(launchResult.status ?? 1);
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

const command = androidArgs[0];
let selectedAndroidDevice = process.env.ANDROID_SERIAL || "";
if ((command === "run" || command === "dev") && !hasExplicitAndroidDevice(androidArgs)) {
  selectedAndroidDevice = detectAndroidDeviceForRun(resolveAdbPath(androidHome));
  const nextPassthroughIndex = androidArgs.indexOf("--");
  if (nextPassthroughIndex === -1) {
    androidArgs.push(selectedAndroidDevice);
  } else {
    androidArgs.splice(nextPassthroughIndex, 0, selectedAndroidDevice);
  }
}

if (selectedAndroidDevice) {
  env.ANDROID_SERIAL = selectedAndroidDevice;
}

if (command === "run") {
  if (!selectedAndroidDevice) {
    selectedAndroidDevice = detectAndroidDeviceForRun(resolveAdbPath(androidHome));
    env.ANDROID_SERIAL = selectedAndroidDevice;
  }

  const buildArgs = [
    "android",
    "build",
    "--debug",
    "--apk",
    "--target",
    "aarch64",
    "--config",
    androidDevConfig,
  ];

  // Keep this custom run path. Tauri CLI 2.x can fall back to emulator
  // discovery even when adb sees a physical USB device. Our project run command
  // must remain one-step and stable: build the debug APK, then install/launch
  // it on the adb-detected phone explicitly.
  const buildResult = spawnSync("tauri", buildArgs, {
    cwd,
    env,
    stdio: "inherit",
  });

  if (buildResult.error) {
    fail(`Failed to launch tauri android build: ${buildResult.error.message}`);
  }

  if (buildResult.status !== 0) {
    process.exit(buildResult.status ?? 1);
  }

  installAndLaunchAndroidDebugApk(resolveAdbPath(androidHome), selectedAndroidDevice);
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
