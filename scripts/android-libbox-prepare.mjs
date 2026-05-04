import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  writeFileSync,
} from "node:fs";
import { execFileSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = fileURLToPath(new URL(".", import.meta.url));
const projectRoot = join(scriptDir, "..");
const appLibsDir = join(projectRoot, "src-tauri", "gen", "android", "app", "libs");
const tempRoot = resolve(
  process.env.RKN_LIBBOX_WORKDIR || "/tmp/rkn-libbox-upstream",
);
const singBoxDir = join(tempRoot, "sing-box");
const sfaDir = join(tempRoot, "sing-box-for-android");
const singBoxRef = process.env.RKN_LIBBOX_SING_BOX_REF || "v1.13.5";
const sfaRef = process.env.RKN_LIBBOX_SFA_REF || "";
const androidPlatform = process.env.RKN_LIBBOX_ANDROID_PLATFORM || "android/arm64";
const includeTailscale = process.env.RKN_LIBBOX_WITH_TAILSCALE === "1";
const requiredAar = join(appLibsDir, "libbox.aar");
const requiredLegacyAar = join(appLibsDir, "libbox-legacy.aar");
const pinnedNdkVersion = "29.0.14206865";

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
  `/usr/local/lib/android/sdk/ndk/${pinnedNdkVersion}`,
  `/opt/android-sdk/ndk/${pinnedNdkVersion}`,
  `/usr/local/share/android-commandlinetools/ndk/${pinnedNdkVersion}`,
  "/usr/local/share/android-ndk",
];

function firstExistingDir(candidates) {
  return candidates.find((candidate) => candidate && existsSync(candidate)) || "";
}

function isSupportedJavaHome(javaHome) {
  if (!javaHome) {
    return false;
  }

  return javaHome.includes("17") || javaHome.includes("21");
}

function isNdkHome(candidate) {
  return Boolean(candidate && existsSync(join(candidate, "source.properties")));
}

function log(message) {
  console.log(`[android:libbox:prepare] ${message}`);
}

function run(command, args, options = {}) {
  log(`> ${command} ${args.join(" ")}`);
  execFileSync(command, args, {
    stdio: "inherit",
    ...options,
  });
}

function capture(command, args, options = {}) {
  return execFileSync(command, args, {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    ...options,
  }).trim();
}

function ensureClone(targetDir, repoUrl, ref = "") {
  if (existsSync(targetDir)) {
    log(`Reusing existing clone: ${targetDir}`);
    if (ref) {
      run("git", ["-C", targetDir, "fetch", "--depth", "1", "origin", ref]);
      run("git", ["-C", targetDir, "checkout", "--detach", "FETCH_HEAD"]);
    } else {
      run("git", ["-C", targetDir, "fetch", "--depth", "1", "origin"]);
      run("git", ["-C", targetDir, "reset", "--hard", "origin/HEAD"]);
    }
    return;
  }

  if (ref) {
    run("git", ["clone", "--depth", "1", "--branch", ref, repoUrl, targetDir]);
  } else {
    run("git", ["clone", "--depth", "1", repoUrl, targetDir]);
  }
}

function ensureToolchainEnv() {
  const fallbackJavaHome = firstExistingDir(fallbackJavaHomes);
  const javaHome = isSupportedJavaHome(process.env.JAVA_HOME || "")
    ? process.env.JAVA_HOME
    : fallbackJavaHome;
  const androidHome =
    process.env.ANDROID_HOME ||
    process.env.ANDROID_SDK_ROOT ||
    fallbackAndroidHome;
  const ndkHome = firstExistingDir([
    process.env.RKN_ANDROID_NDK_HOME,
    androidHome ? join(androidHome, "ndk", pinnedNdkVersion) : "",
    process.env.ANDROID_NDK_HOME,
    process.env.NDK_HOME,
    process.env.NDK,
    ...fallbackNdkHomes,
  ].filter(isNdkHome));

  if (!javaHome) {
    throw new Error("JAVA_HOME is missing or unsupported; no JDK 17/21 fallback was found.");
  }
  if (!androidHome) {
    throw new Error("ANDROID_HOME is missing and no Android SDK fallback was found.");
  }
  if (!ndkHome) {
    throw new Error("NDK_HOME is missing and no Android NDK fallback was found.");
  }

  process.env.JAVA_HOME = javaHome;
  process.env.ANDROID_HOME = androidHome;
  process.env.ANDROID_SDK_ROOT = androidHome;
  process.env.ANDROID_NDK_HOME = ndkHome;
  process.env.NDK_HOME = ndkHome;
  process.env.NDK = ndkHome;
  process.env.PATH = `${join(javaHome, "bin")}:${process.env.PATH}`;

  log(`JAVA_HOME=${javaHome}`);
  log(`ANDROID_HOME=${androidHome}`);
  log(`NDK_HOME=${ndkHome}`);
}

function resolveGoBinDir() {
  const goBin = capture("go", ["env", "GOBIN"]);
  if (goBin) {
    return goBin;
  }

  const goPath = capture("go", ["env", "GOPATH"]);
  return join(goPath, "bin");
}

function resolveBuiltAar(fileName) {
  const candidates = [
    join(singBoxDir, fileName),
    join(sfaDir, "app", "libs", fileName),
  ];

  return candidates.find((candidate) => existsSync(candidate)) || "";
}

function ensureGoTool(command) {
  const binDir = resolveGoBinDir();
  const commandPath = join(binDir, command);

  if (!existsSync(commandPath)) {
    log(`Missing ${command}; installing upstream gomobile toolchain.`);
    run("go", ["install", "-v", "github.com/sagernet/gomobile/cmd/gomobile@v0.1.12"]);
    run("go", ["install", "-v", "github.com/sagernet/gomobile/cmd/gobind@v0.1.12"]);
  }

  if (!process.env.PATH.split(":").includes(binDir)) {
    process.env.PATH = `${binDir}:${process.env.PATH}`;
  }
}

function ensureGomobileInit() {
  log("Running gomobile init for the current toolchain.");
  run("gomobile", ["init"], {
    cwd: singBoxDir,
    env: process.env,
  });
}

function patchSingBoxLibboxBuild() {
  const buildScriptPath = join(singBoxDir, "cmd", "internal", "build_libbox", "main.go");
  let source = readFileSync(buildScriptPath, "utf8");
  let patched = source;

  if (!includeTailscale) {
    const tailscaleTagsLine =
      '\tsharedTags = append(sharedTags, "with_tailscale", "ts_omit_logtail", "ts_omit_ssh", "ts_omit_drive", "ts_omit_taildrop", "ts_omit_webclient", "ts_omit_doctor", "ts_omit_capture", "ts_omit_kube", "ts_omit_aws", "ts_omit_synology", "ts_omit_bird")\n';
    const rknTailscaleComment =
      "\t// RKN Android does not use sing-box Tailscale endpoints; excluding these tags keeps CI deterministic.\n";

    if (patched.includes(tailscaleTagsLine)) {
      patched = patched.replace(tailscaleTagsLine, rknTailscaleComment);
    } else if (patched.includes('"with_tailscale"')) {
      throw new Error(
        "Unable to patch upstream build_libbox Tailscale tags; upstream changed the expected tag line.",
      );
    }
  }

  if (patched !== source) {
    writeFileSync(buildScriptPath, patched);
    log("Patched upstream build_libbox for RKN Android runtime (Tailscale tags disabled).");
  } else {
    log(
      includeTailscale
        ? "Keeping upstream build_libbox Tailscale tags because RKN_LIBBOX_WITH_TAILSCALE=1."
        : "No upstream Tailscale tag patch was needed.",
    );
  }
}

function copyBuiltAar(sourcePath, targetPath) {
  mkdirSync(dirname(targetPath), { recursive: true });
  copyFileSync(sourcePath, targetPath);
  log(`Copied ${sourcePath} -> ${targetPath}`);
}

function main() {
  log("Preparing upstream worktree for libbox build...");
  ensureToolchainEnv();
  mkdirSync(tempRoot, { recursive: true });
  log(`Using sing-box ref: ${singBoxRef}`);
  log(`Using Android libbox platform: ${androidPlatform}`);
  ensureClone(singBoxDir, "https://github.com/SagerNet/sing-box.git", singBoxRef);
  ensureClone(sfaDir, "https://github.com/SagerNet/sing-box-for-android.git", sfaRef);
  patchSingBoxLibboxBuild();

  ensureGoTool("gomobile");
  ensureGoTool("gobind");
  ensureGomobileInit();

  log("Building libbox AARs via official upstream path (go run ./cmd/internal/build_libbox -target android)...");
  run("go", ["run", "./cmd/internal/build_libbox", "-target", "android", "-platform", androidPlatform], {
    cwd: singBoxDir,
    env: process.env,
  });

  const libboxAar = resolveBuiltAar("libbox.aar");
  const libboxLegacyAar = resolveBuiltAar("libbox-legacy.aar");

  if (!libboxAar) {
    throw new Error(
      "libbox.aar was not produced by the upstream build. Check the sing-box build logs above.",
    );
  }

  copyBuiltAar(libboxAar, requiredAar);
  if (libboxLegacyAar) {
    copyBuiltAar(libboxLegacyAar, requiredLegacyAar);
  } else {
    log("libbox-legacy.aar was not found; continuing with the main AAR only.");
  }

  log("Inspecting the copied AAR...");
  run("node", [join(projectRoot, "scripts", "android-libbox-inspect.mjs")], {
    cwd: projectRoot,
    env: process.env,
  });

  const appLibEntries = readdirSync(appLibsDir).sort().join(", ");
  log(`Done. app/libs now contains: ${appLibEntries}`);
}

try {
  main();
} catch (error) {
  console.error(
    `[android:libbox:prepare] FAILED: ${error instanceof Error ? error.message : String(error)}`,
  );
  process.exitCode = 1;
}
