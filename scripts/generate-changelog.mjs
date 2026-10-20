#!/usr/bin/env node
/**
 * Build the release notes published with a GitHub Release.
 *
 * Two sources, in this order of authority:
 *
 *   1. the `## [<version>]` section of `CHANGELOG.md`, which is curated by hand
 *      and is what users read;
 *   2. the Conventional Commit list between the previous tag and HEAD, which is
 *      appended as a collapsed appendix so nobody has to trust the summary.
 *
 * If the version has no changelog section the notes fall back to the commit
 * list alone and say so — a release is never published with a silently empty
 * body.
 *
 * Usage:
 *   node scripts/generate-changelog.mjs > RELEASE_NOTES.md
 *
 * Environment:
 *   GITHUB_REF_NAME     tag being released (`v0.1.0`); falls back to package.json
 *   GITHUB_REPOSITORY   `owner/repo` for commit links
 */

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const ROOT = path.resolve(import.meta.dirname, "..");
const repository = process.env.GITHUB_REPOSITORY ?? "prism-audio/prism";
const repoUrl = `https://github.com/${repository}`;
const tag =
  process.env.GITHUB_REF_NAME ??
  `v${JSON.parse(fs.readFileSync(path.join(ROOT, "package.json"), "utf8")).version}`;
const version = tag.replace(/^v/, "");

function git(args) {
  return execFileSync("git", args, {
    cwd: ROOT,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
}

function previousTag() {
  try {
    return git(["describe", "--tags", "--abbrev=0", `refs/tags/${tag}^`]);
  } catch {
    try {
      return git(["describe", "--tags", "--abbrev=0", "HEAD^"]);
    } catch {
      return null;
    }
  }
}

// ---------------------------------------------------------------------------
//  Changelog section
// ---------------------------------------------------------------------------

/** Return the markdown body of `## [<version>]`, demoted one level for the release page. */
function changelogSection() {
  const file = path.join(ROOT, "CHANGELOG.md");
  if (!fs.existsSync(file)) {
    return null;
  }
  const lines = fs.readFileSync(file, "utf8").split("\n");
  const heading = new RegExp(`^##\\s*\\[?${version.replaceAll(".", "\\.")}\\]?`);
  const start = lines.findIndex((line) => heading.test(line));
  if (start === -1) {
    return null;
  }
  const rest = lines.slice(start + 1);
  const end = rest.findIndex((line) => /^##\s/.test(line));
  const body = (end === -1 ? rest : rest.slice(0, end)).join("\n").trim();
  if (body.length === 0) {
    return "";
  }
  // `### Added` → `## Added`, `#### Foundation` → `### Foundation`: the
  // release page already supplies the H1, so every heading moves up one level.
  return body.replace(/^#{2,5}\s/gm, (match) => match.slice(1));
}

// ---------------------------------------------------------------------------
//  Commit list
// ---------------------------------------------------------------------------

const CATEGORIES = {
  feat: "Features",
  fix: "Fixes",
  perf: "Performance",
  docs: "Documentation",
  refactor: "Refactoring",
  test: "Tests",
  build: "Build & CI",
  ci: "Build & CI",
  style: "Maintenance",
  chore: "Maintenance",
  revert: "Maintenance",
};

const ORDER = [
  "Features",
  "Fixes",
  "Performance",
  "Documentation",
  "Refactoring",
  "Tests",
  "Build & CI",
  "Maintenance",
  "Other changes",
];

function commitGroups(previous) {
  const range = previous === null ? "HEAD" : `${previous}..HEAD`;
  const raw = git(["log", range, "--no-merges", "--pretty=format:%H%x1f%s%x1f%an"]);
  const groups = new Map();
  const commits = raw
    .split("\n")
    .filter((line) => line.length > 0)
    .map((line) => {
      const [hash, subject, author] = line.split("\x1f");
      return { hash, subject, author };
    });

  for (const commit of commits) {
    const parsed = /^(\w+)(?:\(([^)]+)\))?!?:\s*(.+)$/.exec(commit.subject);
    const label = CATEGORIES[parsed?.[1]?.toLowerCase()] ?? "Other changes";
    const scope = parsed?.[2];
    const description = parsed?.[3] ?? commit.subject;
    const line = `- ${scope ? `**${scope}:** ` : ""}${description} ([${commit.hash.slice(0, 7)}](${repoUrl}/commit/${commit.hash})) — ${commit.author}`;
    const bucket = groups.get(label) ?? [];
    bucket.push(line);
    groups.set(label, bucket);
  }
  return { groups, count: commits.length };
}

// ---------------------------------------------------------------------------
//  Render
// ---------------------------------------------------------------------------

const previous = previousTag();
const { groups, count } = commitGroups(previous);

const out = [];
out.push(`# Prism ${tag}`);
out.push("");

const section = changelogSection();
if (section) {
  out.push(section);
  out.push("");
} else {
  out.push(
    previous === null
      ? "First public release."
      : `Changes since [${previous}](${repoUrl}/releases/tag/${previous}).`,
  );
  out.push("");
  out.push(
    `> No curated section for \`${version}\` in \`CHANGELOG.md\`; the notes below are generated from`,
  );
  out.push("> the commit history alone.");
  out.push("");
  for (const label of ORDER) {
    const entries = groups.get(label);
    if (!entries || entries.length === 0) {
      continue;
    }
    out.push(`## ${label}`);
    out.push("");
    out.push(...entries);
    out.push("");
  }
}

if (count > 0) {
  out.push("<details>");
  out.push(`<summary>Commits in this release (${String(count)})</summary>`);
  out.push("");
  for (const label of ORDER) {
    const entries = groups.get(label);
    if (!entries || entries.length === 0) {
      continue;
    }
    out.push(`**${label}**`);
    out.push("");
    out.push(...entries);
    out.push("");
  }
  out.push("</details>");
  out.push("");
}

if (previous !== null) {
  out.push(`**Full changelog:** ${repoUrl}/compare/${previous}...${tag}`);
  out.push("");
}

out.push("**Third-party licences:** `THIRD-PARTY-LICENSES.md` is attached to this release.");
out.push("");

process.stdout.write(out.join("\n"));
