import { existsSync } from "node:fs";
import { join } from "node:path";
import { execFileSync } from "node:child_process";

const cwd = process.cwd();
const apkPath = join(
  cwd,
  "src-tauri",
  "gen",
  "android",
  "app",
  "build",
  "outputs",
  "apk",
  "arm64",
  "debug",
  "app-arm64-debug.apk",
);

if (!existsSync(apkPath)) {
  console.error(`Debug APK not found: ${apkPath}`);
  console.error("Build it first with: npm run tauri:android:run");
  process.exit(1);
}

const devicesOutput = execFileSync("adb", ["devices"], { encoding: "utf8" });
const hasDevice = devicesOutput
  .split("\n")
  .some((line) => /\tdevice$/.test(line.trim()));

if (!hasDevice) {
  console.error("No Android device in adb state 'device'.");
  console.error("Run: adb devices -l");
  process.exit(1);
}

execFileSync("adb", ["install", "-r", apkPath], { stdio: "inherit" });
execFileSync("adb", ["shell", "am", "start", "-n", "com.freedom.rkn/.MainActivity"], {
  stdio: "inherit",
});
