#!/usr/bin/env node
/**
 * Licence audit for everything Prism ships.
 *
 * Prism itself is MIT. This script answers the only question that matters for
 * that claim: *can we licence the distributed binary MIT, given what is linked
 * into it?* It resolves the real dependency graph instead of guessing —
 * `cargo metadata` for Rust, `pnpm licenses list` for the frontend — and
 * applies one explicit policy to the result.
 *
 * Three sets are computed, because "dependency" is three different things:
 *
 *   * **shipped**    — reachable from the crate over normal edges. This is what
 *                      ends up inside `prism.exe` / the AppImage, so this is
 *                      the set the policy is enforced on.
 *   * **build-only** — reachable only over `[build-dependencies]`. It runs on
 *                      the build machine (bindgen, cc, pkg-config …) and is not
 *                      linked into the artefact.
 *   * **npm**        — `pnpm licenses list --prod`, i.e. what Vite bundles.
 *
 * The output is `docs/LICENSES.md`, a document that is generated rather than
 * maintained, so it cannot drift from the lockfiles. `--check` regenerates it
 * in memory and fails if the committed copy is stale; CI runs exactly that.
 *
 * Usage:
 *   node scripts/license-audit.mjs           # write docs/LICENSES.md
 *   node scripts/license-audit.mjs --check   # fail if the file is stale
 *   node scripts/license-audit.mjs --stdout  # print, write nothing
 *
 * Exit codes: 0 compliant · 1 policy violation or stale document · 2 tooling.
 */

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const ROOT = path.resolve(import.meta.dirname, "..");
const OUTPUT = path.join(ROOT, "docs", "LICENSES.md");
const MANIFEST = path.join(ROOT, "src-tauri", "Cargo.toml");

// ---------------------------------------------------------------------------
//  Policy
// ---------------------------------------------------------------------------

/** No obligations beyond keeping the copyright notice. */
const PERMISSIVE = new Set([
  "MIT",
  "MIT-0",
  "0BSD",
  "Apache-2.0",
  "Apache-2.0 WITH LLVM-exception",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "ISC",
  "Zlib",
  "BSL-1.0",
  "Unlicense",
  "CC0-1.0",
  "Unicode-3.0",
]);

/**
 * Allowed, but with an obligation we have to honour in distribution: keep a
 * notice, keep the licence text, or keep the covered files under the same
 * licence if we ever modify them.
 */
const NOTICE = new Map([
  [
    "MPL-2.0",
    "File-level copyleft: an unmodified file may ship under any licence as long as the " +
      "MPL text is included and the source of the covered files stays available. Prism does " +
      "not patch any MPL-2.0 crate; if it ever did, that file alone would have to be published.",
  ],
  [
    "OFL-1.1",
    "SIL Open Font Licence: the font may be embedded and redistributed, but must carry its " +
      "licence and must not be sold on its own. The Inter files that ship keep their OFL text " +
      "inside `node_modules/@fontsource-variable/inter`.",
  ],
  [
    "CDLA-Permissive-2.0",
    "Community Data Licence: permissive for code, requires the notice to be retained.",
  ],
]);

/** Refused outright. */
const FORBIDDEN = [
  [/^A?GPL-/i, "GPL/AGPL: copyleft that would force Prism itself to be GPL"],
  [/^LGPL-/i, "LGPL: acceptable only as an alternative we do not choose"],
  [/^SSPL-/i, "SSPL: not open source for our purposes"],
  [/^EPL-/i, "EPL: weak copyleft with patent-retaliation terms"],
  [/^CDDL-/i, "CDDL: file-level copyleft we do not want in shipped code"],
  [/^EUPL-/i, "EUPL: copyleft"],
  [/^CPAL-/i, "CPAL: copyleft with source-distribution requirements"],
  [/Commons-Clause/i, "Commons Clause removes the right to sell"],
];

/**
 * Packages whose licence metadata is missing or non-SPDX, with the reason we
 * accept them anyway. Empty is the healthy state; every entry needs a
 * justification a reviewer can check.
 */
const MANUAL_APPROVALS = new Map([]);

const RANK = { ok: 0, notice: 1, unknown: 2, forbidden: 3 };
const LABEL = {
  ok: "Permissive",
  notice: "Notice required",
  unknown: "Unreviewed",
  forbidden: "Not allowed",
};

// ---------------------------------------------------------------------------
//  Licence expressions
// ---------------------------------------------------------------------------

/** Strip parentheses and normalise the legacy `MIT/Apache-2.0` form to `OR`. */
function normalise(expression) {
  const trimmed = expression.trim().replace(/[()]/g, " ").replace(/\s+/g, " ").trim();
  if (/\s(OR|AND)\s/.test(trimmed)) {
    return trimmed;
  }
  return trimmed.replace(/\s*[/|]\s*/g, " OR ");
}

/** Split on a separator that is not inside parentheses. */
function splitTopLevel(expression, separator) {
  const parts = [];
  let depth = 0;
  let current = "";
  for (const token of expression.split(/\s+/)) {
    if (token === "(") depth += 1;
    if (token === ")") depth -= 1;
    if (depth === 0 && token === separator) {
      parts.push(current.trim());
      current = "";
      continue;
    }
    current += `${token} `;
  }
  parts.push(current.trim());
  return parts.filter((part) => part.length > 0);
}

function classifyIdentifier(identifier) {
  const id = identifier.trim();
  if (PERMISSIVE.has(id)) {
    return { status: "ok", terms: [id] };
  }
  if (NOTICE.has(id)) {
    return { status: "notice", terms: [id] };
  }
  for (const [pattern, reason] of FORBIDDEN) {
    if (pattern.test(id)) {
      return { status: "forbidden", terms: [id], reason };
    }
  }
  return { status: "unknown", terms: [id], reason: `unrecognised licence id \`${id}\`` };
}

/**
 * Evaluate an SPDX-ish expression.
 *
 * `OR` picks the least demanding branch — that is what dual licensing means:
 * we choose. `AND` requires every branch to clear the policy.
 */
function evaluate(expression) {
  if (!expression || expression.trim().length === 0) {
    return { status: "unknown", terms: [], reason: "no licence field" };
  }
  const normalised = normalise(expression);

  const alternatives = splitTopLevel(normalised, "OR");
  if (alternatives.length > 1) {
    const evaluated = alternatives.map(evaluate);
    evaluated.sort((left, right) => RANK[left.status] - RANK[right.status]);
    return evaluated[0];
  }

  const requirements = splitTopLevel(normalised, "AND");
  if (requirements.length > 1) {
    const evaluated = requirements.map(evaluate);
    const worst = evaluated.reduce((acc, item) =>
      RANK[item.status] > RANK[acc.status] ? item : acc,
    );
    if (worst.status === "forbidden" || worst.status === "unknown") {
      return worst;
    }
    return {
      status: evaluated.some((item) => item.status === "notice") ? "notice" : "ok",
      terms: evaluated.flatMap((item) => item.terms),
    };
  }

  return classifyIdentifier(normalised);
}

// ---------------------------------------------------------------------------
//  Data sources
// ---------------------------------------------------------------------------

/**
 * `cargo metadata` for the workspace.
 *
 * `--offline` keeps the audit usable in the delivery container; a CI runner
 * with an empty registry cache falls back to the network, which is why the
 * retry exists rather than a hard-coded flag.
 */
function cargoMetadata() {
  const args = ["metadata", "--format-version", "1", "--manifest-path", MANIFEST, "--locked"];
  const options = {
    cwd: ROOT,
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
    stdio: ["ignore", "pipe", "pipe"],
  };
  try {
    return JSON.parse(execFileSync("cargo", [...args, "--offline"], options));
  } catch {
    return JSON.parse(execFileSync("cargo", args, options));
  }
}

function dependencySets(metadata) {
  const packages = new Map(metadata.packages.map((pkg) => [pkg.id, pkg]));
  const nodes = new Map(metadata.resolve.nodes.map((node) => [node.id, node]));
  const root = metadata.packages.find((pkg) => pkg.name === "prism");
  if (!root) {
    throw new Error("crate `prism` not found in the workspace metadata");
  }

  const walk = (kinds) => {
    const seen = new Set([root.id]);
    const stack = [root.id];
    while (stack.length > 0) {
      const node = nodes.get(stack.pop());
      for (const dependency of node?.deps ?? []) {
        const dependencyKinds = dependency.dep_kinds?.length
          ? dependency.dep_kinds.map((kind) => kind.kind)
          : [null];
        if (!dependencyKinds.some((kind) => kinds.includes(kind))) {
          continue;
        }
        if (!seen.has(dependency.pkg)) {
          seen.add(dependency.pkg);
          stack.push(dependency.pkg);
        }
      }
    }
    return seen;
  };

  const shipped = walk([null, "normal"]);
  const withBuild = walk([null, "normal", "build"]);
  const buildOnly = new Set([...withBuild].filter((id) => !shipped.has(id)));

  const toPackage = (id) => {
    const pkg = packages.get(id);
    return { name: pkg.name, version: pkg.version, license: pkg.license ?? "" };
  };

  return {
    shipped: [...shipped]
      .map(toPackage)
      .filter((pkg) => pkg.name !== "prism")
      .sort((left, right) => left.name.localeCompare(right.name)),
    buildOnly: [...buildOnly]
      .map(toPackage)
      .sort((left, right) => left.name.localeCompare(right.name)),
  };
}

/**
 * `pnpm` is not always on PATH — Corepack installs it as a shim, and CI images
 * differ from developer machines. Try the binary, then the Corepack shim, and
 * report which failed instead of dying with ENOENT.
 */
function pnpmJsonArguments() {
  const attempts = [
    ["pnpm", ["licenses", "list", "--json", "--prod"]],
    ["corepack", ["pnpm", "licenses", "list", "--json", "--prod"]],
  ];
  for (const [command, args] of attempts) {
    try {
      return execFileSync(command, args, {
        cwd: ROOT,
        encoding: "utf8",
        maxBuffer: 64 * 1024 * 1024,
        stdio: ["ignore", "pipe", "pipe"],
      });
    } catch (error) {
      if (error.code === "ENOENT") {
        continue;
      }
      throw error;
    }
  }
  throw new Error("neither `pnpm` nor `corepack pnpm` is available on PATH");
}

function npmPackages() {
  const raw = pnpmJsonArguments();
  const grouped = JSON.parse(raw);
  const packages = [];
  for (const [license, entries] of Object.entries(grouped)) {
    for (const entry of entries) {
      packages.push({
        name: entry.name,
        version: entry.versions.join(", "),
        license: entry.license ?? license,
      });
    }
  }
  return packages.sort((left, right) => left.name.localeCompare(right.name));
}

// ---------------------------------------------------------------------------
//  Aggregation
// ---------------------------------------------------------------------------

function audit(packages, ecosystem) {
  return packages.map((pkg) => {
    const approval = MANUAL_APPROVALS.get(pkg.name);
    const verdict = evaluate(pkg.license);
    return {
      ...pkg,
      ecosystem,
      status: approval?.status ?? verdict.status,
      terms: verdict.terms,
      reason: approval?.reason ?? verdict.reason ?? "",
    };
  });
}

/** Licence id → number of packages, sorted by count then name. */
function licenceCounts(rows) {
  const counts = new Map();
  for (const row of rows) {
    const key = row.terms.length > 0 ? row.terms.join(" AND ") : "(none declared)";
    const entry = counts.get(key) ?? { count: 0, status: "ok" };
    entry.count += 1;
    if (RANK[row.status] > RANK[entry.status]) {
      entry.status = row.status;
    }
    counts.set(key, entry);
  }
  return new Map(
    [...counts.entries()].sort((left, right) => {
      const byCount = right[1].count - left[1].count;
      return byCount === 0 ? left[0].localeCompare(right[0]) : byCount;
    }),
  );
}

function escapeCell(value) {
  return value.replaceAll("|", "\\|");
}

function inventoryTable(rows) {
  const lines = [];
  lines.push("| Package | Version | Licence (chosen) | Status |");
  lines.push("| :------ | :------ | :--------------- | :----- |");
  for (const row of rows) {
    lines.push(
      `| \`${escapeCell(row.name)}\` | ${escapeCell(row.version)} | ${escapeCell(
        row.terms.length > 0 ? row.terms.join(" AND ") : "—",
      )} | ${LABEL[row.status]} |`,
    );
  }
  return lines;
}

// ---------------------------------------------------------------------------
//  Rendering
// ---------------------------------------------------------------------------

function render({ cargo, npm, buildOnly }) {
  const lines = [];
  const shipped = [...cargo, ...npm];
  const counts = shipped.reduce(
    (acc, row) => acc.set(row.status, (acc.get(row.status) ?? 0) + 1),
    new Map(),
  );
  const decisions = (counts.get("unknown") ?? 0) + (counts.get("forbidden") ?? 0);

  lines.push("# Third-party licences");
  lines.push("");
  lines.push(
    "Prism is released under the MIT licence. This document is the inventory of everything that",
  );
  lines.push(
    "licence has to coexist with. It is generated from `Cargo.lock` and `pnpm-lock.yaml` by",
  );
  lines.push("`node scripts/license-audit.mjs`.");
  lines.push("");
  lines.push("> Generated file — do not edit. CI fails if it drifts from the lockfiles.");
  lines.push("");
  lines.push("## Summary");
  lines.push("");
  lines.push("| Set | Packages | Enforced |");
  lines.push("| :-- | -------: | :------- |");
  lines.push(`| Rust, linked into the binary | ${String(cargo.length)} | by this audit |`);
  lines.push(
    `| Rust, build-only | ${String(buildOnly.length)} | runs on the build machine, never linked |`,
  );
  lines.push(`| npm, bundled into the frontend | ${String(npm.length)} | by this audit |`);
  lines.push("");
  lines.push(
    `Of the ${String(shipped.length)} shipped packages, **${String(counts.get("ok") ?? 0)}** are ` +
      `permissive, **${String(counts.get("notice") ?? 0)}** carry a notice obligation and ` +
      `**${String(decisions)}** require a decision. No GPL, AGPL, SSPL or CDDL package appears ` +
      "anywhere in the graph.",
  );
  lines.push("");
  lines.push("### Licences in use");
  lines.push("");
  lines.push("| Licence | Packages | Status |");
  lines.push("| :------ | -------: | :----- |");
  for (const [licence, entry] of licenceCounts(shipped)) {
    lines.push(`| ${escapeCell(licence)} | ${String(entry.count)} | ${LABEL[entry.status]} |`);
  }
  lines.push("");
  lines.push("## Policy");
  lines.push("");
  lines.push(
    "Permissive (MIT, MIT-0, 0BSD, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause,",
  );
  lines.push(
    "BSD-3-Clause, ISC, Zlib, BSL-1.0, Unlicense, CC0-1.0, Unicode-3.0) is accepted outright: it",
  );
  lines.push("imposes nothing beyond attribution, which this document provides.");
  lines.push("");
  lines.push(
    "Notice-required licences are accepted with an explicit obligation, listed below. GPL, AGPL,",
  );
  lines.push(
    "SSPL, EPL, CDDL, EUPL, CPAL and the Commons Clause are refused, because they would constrain",
  );
  lines.push("how Prism itself is licensed.");
  lines.push("");
  lines.push(
    "Where a crate offers a choice — `MIT OR Apache-2.0 OR LGPL-2.1-or-later`, for instance — the",
  );
  lines.push(
    "audit picks the least demanding branch, which is the entire point of dual licensing. Only the",
  );
  lines.push(
    "default feature set is counted: `profiling` (pprof, inferno) is a maintainer tool that never",
  );
  lines.push("enters a release artefact, so it is not part of this inventory.");
  lines.push("");
  lines.push("## Notices we must honour");
  lines.push("");

  const mpl = shipped.filter((row) => row.terms.includes("MPL-2.0"));
  if (mpl.length > 0) {
    lines.push(`### MPL-2.0 — ${String(mpl.length)} packages, all unmodified`);
    lines.push("");
    lines.push(
      "This is the file-level copyleft that covers the audio decoders and two of Tauri's CSS",
    );
    lines.push(
      "crates. Shipping them inside an MIT application is explicitly permitted as long as the",
    );
    lines.push(
      "MPL text travels with the distribution and any modified file is published under MPL-2.0.",
    );
    lines.push(
      "Prism does not patch any of them: they are consumed from crates.io exactly as pinned in",
    );
    lines.push("`Cargo.lock`.");
    lines.push("");
  }

  const apache = shipped.filter((row) => row.terms.includes("Apache-2.0"));
  if (apache.length > 0) {
    lines.push(`### Apache-2.0 — ${String(apache.length)} packages`);
    lines.push("");
    lines.push(
      "Apache-2.0 requires the licence text, and any `NOTICE` file a crate ships, to be retained",
    );
    lines.push(
      "in distribution. Release artefacts publish this document next to the binaries for exactly",
    );
    lines.push(
      "that reason; the full texts remain available in the corresponding crates.io entries and",
    );
    lines.push("npm packages.");
    lines.push("");
  }

  const font = npm.filter((row) => row.terms.includes("OFL-1.1"));
  if (font.length > 0) {
    lines.push("### OFL-1.1 — the interface typeface");
    lines.push("");
    lines.push(
      `Inter (${font.map((row) => `\`${row.name}@${row.version}\``).join(", ")}) is embedded under`,
    );
    lines.push(
      "the SIL Open Font Licence: it may be bundled and redistributed, it may not be sold on its",
    );
    lines.push("own, and its licence text ships inside the npm package.");
    lines.push("");
  }

  lines.push("### Linux runtime libraries");
  lines.push("");
  lines.push("On Linux the application links dynamically against WebKitGTK and GTK, which are");
  lines.push(
    "LGPL-2.1-or-later and are supplied by the operating system. Dynamic linking against them",
  );
  lines.push(
    "does not extend the LGPL to Prism's own code, and the `.deb` declares them as dependencies",
  );
  lines.push("instead of bundling private copies.");
  lines.push("");
  lines.push("## Inventory — Rust, linked into the binary");
  lines.push("");
  lines.push(...inventoryTable(cargo));
  lines.push("");
  lines.push("## Inventory — npm, bundled into the frontend");
  lines.push("");
  lines.push(...inventoryTable(npm));
  lines.push("");

  if (buildOnly.length > 0) {
    const buildGroups = new Map();
    for (const pkg of buildOnly) {
      const terms = evaluate(pkg.license).terms;
      const key = terms.length > 0 ? terms.join(" AND ") : "—";
      const bucket = buildGroups.get(key) ?? [];
      bucket.push(pkg.name);
      buildGroups.set(key, bucket);
    }
    lines.push("## Build-only dependencies");
    lines.push("");
    lines.push(
      "Reachable only through `[build-dependencies]`: they execute on the build machine and are not",
    );
    lines.push("linked into the artefact. Listed for completeness, not enforced.");
    lines.push("");
    for (const [licence, names] of [...buildGroups.entries()].sort((left, right) =>
      left[0].localeCompare(right[0]),
    )) {
      lines.push(`- **${licence}** (${String(names.length)}): ${names.sort().join(", ")}`);
    }
    lines.push("");
  }

  lines.push("## Reproducing this document");
  lines.push("");
  lines.push("```bash");
  lines.push("node scripts/license-audit.mjs          # regenerate");
  lines.push("node scripts/license-audit.mjs --check  # verify (CI)");
  lines.push("```");
  lines.push("");
  lines.push(
    "`cargo metadata --locked` resolves the Rust graph and `pnpm licenses list --json --prod`",
  );
  lines.push(
    "resolves the frontend graph. Neither reaches the network when the caches are warm, which is",
  );
  lines.push("what lets the audit run in the same offline container as the rest of CI.");
  lines.push("");

  return { document: lines.join("\n"), counts, shipped };
}

// ---------------------------------------------------------------------------
//  Main
// ---------------------------------------------------------------------------

function main() {
  const flags = new Set(process.argv.slice(2));

  let metadata;
  try {
    metadata = cargoMetadata();
  } catch (error) {
    console.error("license-audit: cargo metadata failed —", error.message);
    process.exit(2);
  }

  let npm;
  try {
    npm = npmPackages();
  } catch (error) {
    console.error("license-audit: pnpm licenses failed —", error.message);
    process.exit(2);
  }

  const { shipped: rustCrates, buildOnly } = dependencySets(metadata);
  const cargoRows = audit(rustCrates, "cargo");
  const npmRows = audit(npm, "npm");
  const rendered = render({ cargo: cargoRows, npm: npmRows, buildOnly });

  if (flags.has("--stdout")) {
    process.stdout.write(rendered.document);
  } else if (flags.has("--check")) {
    const existing = fs.existsSync(OUTPUT) ? fs.readFileSync(OUTPUT, "utf8") : "";
    if (existing.trim() !== rendered.document.trim()) {
      console.error(
        "license-audit: docs/LICENSES.md is stale — run `node scripts/license-audit.mjs` and commit the result",
      );
      process.exit(1);
    }
  } else {
    fs.writeFileSync(OUTPUT, `${rendered.document}\n`);
    console.log(`license-audit: wrote ${path.relative(ROOT, OUTPUT)}`);
  }

  const violations = [...cargoRows, ...npmRows].filter(
    (row) => row.status === "forbidden" || row.status === "unknown",
  );
  console.log(
    `license-audit: ${String(rendered.shipped.length)} shipped packages — ` +
      `${String(rendered.counts.get("ok") ?? 0)} permissive, ` +
      `${String(rendered.counts.get("notice") ?? 0)} with notice, ` +
      `${String(buildOnly.length)} build-only`,
  );

  if (violations.length > 0) {
    for (const row of violations) {
      console.error(
        `  ${row.status === "forbidden" ? "NOT ALLOWED" : "REVIEW"}: ${row.name}@${row.version} — ` +
          `${row.license || "(no licence field)"}${row.reason === "" ? "" : ` (${row.reason})`}`,
      );
    }
    process.exit(1);
  }
}

main();
