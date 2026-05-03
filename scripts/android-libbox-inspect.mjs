import { existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { join } from "node:path";

const scriptDir = fileURLToPath(new URL(".", import.meta.url));
const projectRoot = join(scriptDir, "..");
const aarPath = join(
  projectRoot,
  "src-tauri",
  "gen",
  "android",
  "app",
  "libs",
  "libbox.aar",
);

if (!existsSync(aarPath)) {
  console.error(`[MISS] libbox.aar not found at ${aarPath}`);
  process.exit(1);
}

let entries = [];

try {
  const output = execFileSync("jar", ["tf", aarPath], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
  entries = output
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
} catch (error) {
  console.error(
    `[MISS] Failed to inspect libbox.aar with 'jar tf'. ${error instanceof Error ? error.message : String(error)}`,
  );
  process.exit(1);
}

const classesJar = entries.includes("classes.jar");
const nativeLibs = entries.filter((entry) => entry.startsWith("jni/"));
const arm64Libs = nativeLibs.filter((entry) => entry.startsWith("jni/arm64-v8a/"));
const classCandidates = entries.filter(
  (entry) =>
    entry.includes("libbox") ||
    entry.includes("nekohasekai") ||
    entry.includes("sagernet"),
);

console.log(`[OK ] libbox.aar: ${aarPath}`);
console.log(`[INFO] classes.jar: ${classesJar ? "present" : "missing"}`);
console.log(`[INFO] native libs: ${nativeLibs.length > 0 ? nativeLibs.length : "none"}`);

if (arm64Libs.length > 0) {
  console.log("[INFO] arm64 libs:");
  for (const entry of arm64Libs) {
    console.log(`  - ${entry}`);
  }
} else {
  console.log("[INFO] arm64 libs: none");
}

if (classCandidates.length > 0) {
  console.log("[INFO] relevant package/class entries:");
  for (const entry of classCandidates.slice(0, 50)) {
    console.log(`  - ${entry}`);
  }
} else {
  console.log("[INFO] relevant package/class entries: none matched heuristic");
}
