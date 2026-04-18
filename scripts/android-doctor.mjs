import { accessSync, constants, existsSync } from "node:fs";
import { join } from "node:path";
import { execFileSync } from "node:child_process";

const cwd = process.cwd();
const checks = [];

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
const fallbackNdkHome = existsSync("/usr/local/share/android-ndk/source.properties")
  ? "/usr/local/share/android-ndk"
  : "";
const fallbackAndroidNdkHome = existsSync(
  "/usr/local/share/android-commandlinetools/ndk/29.0.14206865/source.properties",
)
  ? "/usr/local/share/android-commandlinetools/ndk/29.0.14206865"
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

function firstExistingDir(candidates) {
  return candidates.find((candidate) => candidate && existsSync(candidate)) || "";
}

function isAndroidCompatibleJavaHome(javaHome) {
  if (!javaHome) {
    return false;
  }

  return !javaHome.includes("temurin-26");
}

const fallbackJavaHome = firstExistingDir(fallbackJavaHomes);
const javaHome =
  isAndroidCompatibleJavaHome(process.env.JAVA_HOME || "")
    ? process.env.JAVA_HOME
    : fallbackJavaHome;
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

const ndkHome = firstExistingDir([
  process.env.NDK_HOME,
  process.env.ANDROID_NDK_HOME,
  fallbackNdkHome,
  fallbackAndroidNdkHome,
]);
pushCheck(
  "NDK_HOME",
  Boolean(ndkHome) && existsSync(join(ndkHome, "source.properties")),
  ndkHome || "not set",
);

pushCheck("java", commandExists("java"), commandExists("java") ? "ok" : "missing");
const javaLooksCompatible = isAndroidCompatibleJavaHome(javaHome);
pushCheck(
  "Android Java compatibility",
  javaLooksCompatible,
  javaHome
    ? javaLooksCompatible
      ? `${javaHome} (looks compatible for current Gradle)`
      : `${javaHome} (too new for current Android Gradle path; prefer JDK 17 or 21)`
    : "not set",
);
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

const androidRanlib =
  process.env.TARGET_RANLIB ||
  process.env.RANLIB_aarch64_linux_android ||
  join(
    ndkHome || "",
    "toolchains",
    "llvm",
    "prebuilt",
    "darwin-x86_64",
    "bin",
    "llvm-ranlib",
  );

pushCheck(
  "Android llvm-ranlib",
  Boolean(androidRanlib) && existsSync(androidRanlib),
  androidRanlib || "not set",
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
