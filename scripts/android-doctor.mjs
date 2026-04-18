import { accessSync, constants, existsSync } from "node:fs";
import { join } from "node:path";
import { execFileSync } from "node:child_process";

const cwd = process.cwd();
const checks = [];

const fallbackJavaHome = existsSync("/Library/Java/JavaVirtualMachines/temurin-26.jdk/Contents/Home")
  ? "/Library/Java/JavaVirtualMachines/temurin-26.jdk/Contents/Home"
  : "";
const fallbackAndroidHome = existsSync("/usr/local/share/android-commandlinetools")
  ? "/usr/local/share/android-commandlinetools"
  : "";
const fallbackNdkHome = existsSync("/usr/local/share/android-ndk/source.properties")
  ? "/usr/local/share/android-ndk"
  : "";

function commandExists(command, args = ["--version"]) {
  try {
    execFileSync(command, args, { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

function pushCheck(name, ok, details) {
  checks.push({ name, ok, details });
}

const javaHome = process.env.JAVA_HOME || fallbackJavaHome;
pushCheck(
  "JAVA_HOME",
  Boolean(javaHome) && existsSync(javaHome),
  javaHome || "not set",
);

const androidHome =
  process.env.ANDROID_HOME || process.env.ANDROID_SDK_ROOT || fallbackAndroidHome;
pushCheck(
  "ANDROID_HOME",
  Boolean(androidHome) && existsSync(androidHome),
  androidHome || "not set",
);

const ndkHome = process.env.NDK_HOME || process.env.ANDROID_NDK_HOME || fallbackNdkHome;
pushCheck(
  "NDK_HOME",
  Boolean(ndkHome) && existsSync(join(ndkHome, "source.properties")),
  ndkHome || "not set",
);

pushCheck("java", commandExists("java"), commandExists("java") ? "ok" : "missing");
pushCheck(
  "adb",
  commandExists("adb", ["version"]),
  commandExists("adb", ["version"]) ? "ok" : "missing",
);
pushCheck(
  "sdkmanager",
  commandExists("sdkmanager", ["--version"]),
  commandExists("sdkmanager", ["--version"]) ? "ok" : "missing",
);

const androidScaffoldRoot = join(cwd, "src-tauri", "gen", "android");
pushCheck(
  "Android scaffold",
  existsSync(androidScaffoldRoot),
  androidScaffoldRoot,
);

const androidSidecar = join(
  cwd,
  "src-tauri",
  "bins",
  "sing-box-aarch64-linux-android",
);

let androidSidecarExecutable = false;
if (existsSync(androidSidecar)) {
  try {
    accessSync(androidSidecar, constants.X_OK);
    androidSidecarExecutable = true;
  } catch {
    androidSidecarExecutable = false;
  }
}

pushCheck(
  "Android sing-box sidecar",
  existsSync(androidSidecar),
  existsSync(androidSidecar)
    ? androidSidecarExecutable
      ? `${androidSidecar} (executable)`
      : `${androidSidecar} (present, permissions not verified on this host)`
    : `${androidSidecar} (missing)`,
);

const hasFailure = checks.some((check) => !check.ok);

for (const check of checks) {
  const marker = check.ok ? "OK " : "MISS";
  console.log(`[${marker}] ${check.name}: ${check.details}`);
}

if (hasFailure) {
  process.exitCode = 1;
}
