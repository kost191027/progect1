import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.resolve(__dirname, "..");

const packageJsonPath = path.join(rootDir, "package.json");
const packageLockPath = path.join(rootDir, "package-lock.json");
const tauriConfigPath = path.join(rootDir, "src-tauri", "tauri.conf.json");
const cargoTomlPath = path.join(rootDir, "src-tauri", "Cargo.toml");

const semverPattern = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/;

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function writeJson(filePath, value) {
  fs.writeFileSync(filePath, `${JSON.stringify(value, null, 2)}\n`);
}

function readText(filePath) {
  return fs.readFileSync(filePath, "utf8");
}

function writeText(filePath, value) {
  fs.writeFileSync(filePath, value);
}

function parseSemver(version) {
  const match = version.match(/^(\d+)\.(\d+)\.(\d+)(-.+)?$/);

  if (!match) {
    throw new Error(`Unsupported version format: ${version}`);
  }

  return {
    major: Number(match[1]),
    minor: Number(match[2]),
    patch: Number(match[3]),
    suffix: match[4] ?? "",
  };
}

function bumpVersion(currentVersion, releaseType) {
  const { major, minor, patch } = parseSemver(currentVersion);

  switch (releaseType) {
    case "patch":
      return `${major}.${minor}.${patch + 1}`;
    case "minor":
      return `${major}.${minor + 1}.0`;
    case "major":
      return `${major + 1}.0.0`;
    default:
      return releaseType;
  }
}

function validateVersion(version) {
  if (!semverPattern.test(version)) {
    throw new Error(
      `Version "${version}" is invalid. Use semver like 0.1.1 or 1.0.0-beta.1.`,
    );
  }
}

function getVersions() {
  const packageJson = readJson(packageJsonPath);
  const packageLock = readJson(packageLockPath);
  const tauriConfig = readJson(tauriConfigPath);
  const cargoToml = readText(cargoTomlPath);
  const cargoMatch = cargoToml.match(/^\[package\][\s\S]*?^version = "([^"]+)"/m);

  if (!cargoMatch) {
    throw new Error("Unable to find [package] version in src-tauri/Cargo.toml");
  }

  return {
    packageJson: packageJson.version,
    packageLock: packageLock.version,
    packageLockRoot: packageLock.packages?.[""]?.version,
    tauriConfig: tauriConfig.version,
    cargoToml: cargoMatch[1],
  };
}

function syncVersion(targetVersion) {
  validateVersion(targetVersion);

  const packageJson = readJson(packageJsonPath);
  packageJson.version = targetVersion;
  writeJson(packageJsonPath, packageJson);

  const packageLock = readJson(packageLockPath);
  packageLock.version = targetVersion;
  if (packageLock.packages?.[""]) {
    packageLock.packages[""].version = targetVersion;
  }
  writeJson(packageLockPath, packageLock);

  const tauriConfig = readJson(tauriConfigPath);
  tauriConfig.version = targetVersion;
  writeJson(tauriConfigPath, tauriConfig);

  const cargoToml = readText(cargoTomlPath);
  const nextCargoToml = cargoToml.replace(
    /^(\[package\][\s\S]*?^version = ")([^"]+)(")/m,
    `$1${targetVersion}$3`,
  );
  writeText(cargoTomlPath, nextCargoToml);
}

function checkVersions() {
  const versions = getVersions();
  const expected = versions.packageJson;
  const mismatches = Object.entries(versions).filter(([, value]) => value !== expected);

  if (mismatches.length === 0) {
    console.log(`Versions are synchronized: ${expected}`);
    return;
  }

  console.error(`Version mismatch detected. package.json = ${expected}`);
  for (const [label, value] of mismatches) {
    console.error(`- ${label}: ${value}`);
  }
  process.exit(1);
}

function printUsage() {
  console.log(`Usage:
  npm run version:sync
  npm run version:check
  npm run version:bump -- <version | patch | minor | major>`);
}

function main() {
  const [, , command, input] = process.argv;

  if (!command) {
    printUsage();
    process.exit(1);
  }

  if (command === "sync") {
    const target = getVersions().packageJson;
    syncVersion(target);
    console.log(`Synchronized project version to ${target}`);
    return;
  }

  if (command === "check") {
    checkVersions();
    return;
  }

  if (command === "bump") {
    if (!input) {
      printUsage();
      process.exit(1);
    }

    const current = getVersions().packageJson;
    const next = bumpVersion(current, input);
    validateVersion(next);
    syncVersion(next);
    console.log(`Bumped project version: ${current} -> ${next}`);
    return;
  }

  printUsage();
  process.exit(1);
}

main();
