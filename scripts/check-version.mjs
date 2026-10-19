#!/usr/bin/env node
/**
 * Verify that every version carrier agrees before a release is published.
 *
 * Sources of truth checked:
 *   * package.json           → `version`
 *   * Cargo.toml (workspace) → `[workspace.package] version`
 *   * src-tauri/Cargo.toml   → must inherit via `version.workspace = true`
 *   * tauri.conf.json        → `version`
 *   * optional git tag        → argv[2] or GITHUB_REF_NAME, leading `v` stripped
 *
 * Usage: node scripts/check-version.mjs [tag]
 * Exit code 0 when consistent, 1 otherwise (CI-friendly diagnostics).
 */
import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const root = path.resolve(import.meta.dirname, "..");
const failures = [];

function readJson(relative) {
  return JSON.parse(fs.readFileSync(path.join(root, relative), "utf8"));
}

const packageJson = readJson("package.json");
const tauriConf = readJson("src-tauri/tauri.conf.json");

const workspaceCargo = fs.readFileSync(path.join(root, "Cargo.toml"), "utf8");
const packageSection = workspaceCargo.match(/\[workspace\.package\]([\s\S]*?)(?:\n\[|$)/);
if (!packageSection) {
  failures.push("Cargo.toml: [workspace.package] section not found");
}
const cargoVersion = packageSection?.[1].match(/^\s*version\s*=\s*"([^"]+)"/m)?.[1];

const crateCargo = fs.readFileSync(path.join(root, "src-tauri/Cargo.toml"), "utf8");
if (!/^\s*version\.workspace\s*=\s*true\s*$/m.test(crateCargo)) {
  failures.push("src-tauri/Cargo.toml must inherit `version.workspace = true`");
}

const versions = {
  "package.json": packageJson.version,
  "Cargo.toml": cargoVersion,
  "tauri.conf.json": tauriConf.version,
};

const declared = new Set(Object.values(versions));
if (declared.size !== 1) {
  failures.push(
    `version mismatch → ${Object.entries(versions)
      .map(([file, version]) => `${file}=${String(version)}`)
      .join(", ")}`,
  );
}

const tag = process.argv[2] ?? process.env.GITHUB_REF_NAME;
if (tag) {
  const normalised = tag.replace(/^v/, "");
  if (normalised !== packageJson.version) {
    failures.push(`tag '${tag}' does not match package version '${packageJson.version}'`);
  }
}

if (failures.length > 0) {
  console.error("✖ Version verification failed:");
  for (const failure of failures) {
    console.error(`  - ${failure}`);
  }
  process.exit(1);
}

console.log(`✔ Version ${packageJson.version} is consistent across all manifests`);
