#!/usr/bin/env node
/**
 * Build structured release notes from Conventional Commits.
 *
 * Categories: Features, Fixes, Performance, Documentation, Refactoring, Tests,
 * Build & CI, Maintenance. Commits are linked to GitHub when GITHUB_REPOSITORY
 * is available. The previous tag is discovered with `git describe`; a first
 * release simply lists everything reachable from HEAD.
 *
 * Usage: node scripts/generate-changelog.mjs > RELEASE_NOTES.md
 */
import { execFileSync } from "node:child_process";
import process from "node:process";

const repository = process.env.GITHUB_REPOSITORY ?? "prism-audio/prism";
const repoUrl = `https://github.com/${repository}`;

function git(args) {
  return execFileSync("git", args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}

function previousTag() {
  try {
    return git(["describe", "--tags", "--abbrev=0", "HEAD^"]);
  } catch {
    return null;
  }
}

const categories = [
  ["feat", "Features"],
  ["fix", "Fixes"],
  ["perf", "Performance"],
  ["docs", "Documentation"],
  ["refactor", "Refactoring"],
  ["test", "Tests"],
  ["build", "Build & CI"],
  ["ci", "Build & CI"],
  ["style", "Maintenance"],
  ["chore", "Maintenance"],
  ["revert", "Maintenance"],
];

const order = [
  "Features",
  "Fixes",
  "Performance",
  "Documentation",
  "Refactoring",
  "Tests",
  "Build & CI",
  "Maintenance",
  "Other Changes",
];

const previous = previousTag();
const range = previous === null ? "HEAD" : `${previous}..HEAD`;

const raw = git(["log", range, "--no-merges", "--pretty=format:%H%x1f%s%x1f%an"]);
const commits = raw
  .split("\n")
  .filter((line) => line.length > 0)
  .map((line) => {
    const [hash, subject, author] = line.split("\x1f");
    return { hash, subject, author };
  });

const groups = new Map();
for (const commit of commits) {
  const parsed = /^(\w+)(?:\(([^)]+)\))?!?:\s*(.+)$/.exec(commit.subject);
  const type = parsed?.[1]?.toLowerCase();
  const scope = parsed?.[2];
  const description = parsed?.[3] ?? commit.subject;
  const label = categories.find(([key]) => key === type)?.[1] ?? "Other Changes";

  const line = `- ${scope ? `**${scope}:** ` : ""}${description} ([${commit.hash.slice(0, 7)}](${repoUrl}/commit/${commit.hash})) — ${commit.author}`;
  const bucket = groups.get(label) ?? [];
  bucket.push(line);
  groups.set(label, bucket);
}

const lines = [];
lines.push(`# Prism ${process.env.GITHUB_REF_NAME ?? ""}`.trim());
lines.push("");
lines.push(
  previous === null
    ? "Initial public release."
    : `Changes since [${previous}](${repoUrl}/releases/tag/${previous}).`,
);
lines.push("");

for (const label of order) {
  const entries = groups.get(label);
  if (!entries || entries.length === 0) {
    continue;
  }
  lines.push(`## ${label}`);
  lines.push("");
  lines.push(...entries);
  lines.push("");
}

if (commits.length === 0) {
  lines.push("## Maintenance");
  lines.push("");
  lines.push("- Re-release with no code changes.");
  lines.push("");
}

if (previous !== null) {
  lines.push(
    `**Full changelog:** ${repoUrl}/compare/${previous}...${process.env.GITHUB_REF_NAME ?? "HEAD"}`,
  );
  lines.push("");
}

lines.push(`**Repository:** ${repoUrl}`);
lines.push("");

process.stdout.write(lines.join("\n"));
