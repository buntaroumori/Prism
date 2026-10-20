#!/usr/bin/env python3
"""Static validation of the release pipeline.

A release workflow cannot be exercised on a laptop: it needs two operating
systems, code signing, and a tag that does not exist yet. What *can* be checked
is everything the workflow assumes — and those assumptions are exactly what
breaks a release at 2 a.m.:

* the tag, `package.json`, the Cargo workspace and `tauri.conf.json` agree;
* every bundling target in `tauri.conf.json` has a staging step, and every
  artefact those steps stage is uploaded and published;
* every local script a step calls exists;
* every third-party action is pinned to a version or a commit, never to a
  moving branch;
* the changelog has a section for the version being released;
* the licence audit still passes and `docs/LICENSES.md` is current.

Usage:
    python3 scripts/validate_release.py            # version from package.json
    python3 scripts/validate_release.py v0.2.0     # validate a specific tag
    python3 scripts/validate_release.py --skip-licence

Exit code 0 = the pipeline is internally consistent. Exit code 1 = a check
failed, printed with the file and the reason.
"""

from __future__ import annotations

import fnmatch
import json
import re
import subprocess
import sys
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parent.parent
RELEASE = ROOT / ".github" / "workflows" / "release.yml"

# Actions that legitimately track a branch: the toolchain selector *is* the
# branch name, so nothing is pinned by design.
ACTION_EXCEPTIONS = {
    "dtolnay/rust-toolchain@master": "selects the toolchain from `with: toolchain`",
}

# Directories tauri-bundler writes each artefact family into, keyed by the
# `bundle.targets` identifier.
TARGET_DIRECTORIES = {
    "msi": "bundle/msi",
    "nsis": "bundle/nsis",
    "appimage": "bundle/appimage",
    "deb": "bundle/deb",
}


class Report:
    def __init__(self) -> None:
        self.rows: list[tuple[str, bool, str]] = []

    def add(self, name: str, ok: bool, detail: str = "") -> None:
        self.rows.append((name, ok, detail))

    @property
    def failed(self) -> bool:
        return any(not ok for _, ok, _ in self.rows)

    def render(self) -> str:
        width = max(len(name) for name, _, _ in self.rows) + 2
        lines = []
        for name, ok, detail in self.rows:
            mark = "ok  " if ok else "FAIL"
            lines.append(f"[{mark}] {name.ljust(width)}{detail}")
        return "\n".join(lines)


def load_workflow(report: Report) -> dict:
    try:
        workflow = yaml.safe_load(RELEASE.read_text())
    except FileNotFoundError:
        report.add("release workflow", False, ".github/workflows/release.yml is missing")
        return {}
    except yaml.YAMLError as error:  # pragma: no cover - defensive
        report.add("release workflow", False, f"invalid YAML: {error}")
        return {}
    report.add("release workflow", True, "parses as YAML")
    return workflow


def all_steps(workflow: dict) -> list[tuple[str, dict]]:
    steps: list[tuple[str, dict]] = []
    for job_name, job in (workflow.get("jobs") or {}).items():
        for step in job.get("steps") or []:
            steps.append((job_name, step))
    return steps


def check_versions(report: Report, tag: str) -> str:
    version = tag.removeprefix("v")
    package = json.loads((ROOT / "package.json").read_text())
    tauri = json.loads((ROOT / "src-tauri" / "tauri.conf.json").read_text())
    cargo = (ROOT / "Cargo.toml").read_text()
    match = re.search(r'^version\s*=\s*"([^"]+)"', cargo.split("[workspace.package]")[1], re.MULTILINE)
    cargo_version = match.group(1) if match else "?"

    carriers = {
        "package.json": package.get("version"),
        "Cargo.toml": cargo_version,
        "tauri.conf.json": tauri.get("version"),
    }
    consistent = len(set(carriers.values())) == 1
    report.add(
        "version carriers agree",
        consistent,
        ", ".join(f"{name}={value}" for name, value in carriers.items()),
    )
    report.add(
        "tag matches manifests",
        version == package.get("version"),
        f"tag {tag} vs package {package.get('version')}",
    )
    return version


def check_trigger(report: Report, workflow: dict) -> None:
    triggers = workflow.get("on") or workflow.get(True) or {}
    tags = (triggers.get("push") or {}).get("tags") or []
    report.add(
        "triggers on version tags",
        "v*" in tags,
        f"push.tags = {tags}",
    )
    permissions = workflow.get("permissions") or {}
    report.add(
        "release permission",
        permissions.get("contents") == "write",
        "contents: write",
    )


def check_job_graph(report: Report, workflow: dict) -> None:
    jobs = workflow.get("jobs") or {}
    expected = ["quality", "bundle", "publish"]
    report.add("jobs present", all(name in jobs for name in expected), ", ".join(jobs))

    bundle_needs = jobs.get("bundle", {}).get("needs")
    publish_needs = jobs.get("publish", {}).get("needs")
    report.add(
        "quality gates the bundle",
        bundle_needs == "quality",
        f"bundle.needs = {bundle_needs}",
    )
    report.add(
        "bundles gate the publish",
        publish_needs == "bundle",
        f"publish.needs = {publish_needs}",
    )

    platforms = {
        entry.get("platform")
        for entry in (jobs.get("bundle", {}).get("strategy") or {}).get("matrix", {}).get("include", [])
    }
    report.add(
        "both operating systems bundled",
        {"windows", "linux"} <= platforms,
        f"platforms = {sorted(platform for platform in platforms if platform)}",
    )

    concurrency = workflow.get("concurrency") or {}
    report.add(
        "releases never cancel each other",
        concurrency.get("cancel-in-progress") is False,
        "concurrency.cancel-in-progress = false",
    )


def check_action_pinning(report: Report, workflow: dict) -> None:
    unpinned = []
    for job_name, step in all_steps(workflow):
        uses = step.get("uses")
        if not uses:
            continue
        if uses in ACTION_EXCEPTIONS:
            continue
        if re.search(r"@[0-9a-f]{40}$", uses) or re.search(r"@v\d+(\.\d+)*$", uses):
            continue
        unpinned.append(f"{job_name}: {uses}")
    report.add(
        "actions pinned to versions",
        not unpinned,
        "ok" if not unpinned else "; ".join(unpinned),
    )


def check_scripts_exist(report: Report, workflow: dict) -> None:
    missing = []
    for job_name, step in all_steps(workflow):
        command = step.get("run") or ""
        for match in re.finditer(r"(?:node|python3)\s+(scripts/[\w.\-]+)", command):
            if not (ROOT / match.group(1)).is_file():
                missing.append(f"{job_name}: {match.group(1)}")
    report.add(
        "referenced scripts exist",
        not missing,
        "ok" if not missing else "; ".join(sorted(set(missing))),
    )


def check_secrets(report: Report, workflow: dict) -> None:
    problems = []
    for job_name, step in all_steps(workflow):
        condition = step.get("if") or ""
        if "secrets." in condition:
            problems.append(f"{job_name}: secrets in `if:` ({step.get('name', '?')})")
        if step.get("continue-on-error") is True:
            problems.append(f"{job_name}: continue-on-error ({step.get('name', '?')})")
    report.add(
        "no secrets in conditions, no ignored failures",
        not problems,
        "ok" if not problems else "; ".join(problems),
    )


def check_artefacts(report: Report, workflow: dict) -> None:
    tauri = json.loads((ROOT / "src-tauri" / "tauri.conf.json").read_text())
    targets = tauri.get("bundle", {}).get("targets") or []
    steps = all_steps(workflow)
    publish = next((step for _, step in steps if step.get("uses", "").startswith("softprops/action-gh-release")), None)

    if publish is None:
        report.add("publish step", False, "no softprops/action-gh-release step")
        return

    published = [line.strip() for line in (publish.get("with", {}).get("files") or "").splitlines() if line.strip()]
    bodies = "\n".join(step.get("run") or "" for _, step in steps)

    for target in targets:
        directory = TARGET_DIRECTORIES.get(target)
        if directory is None:
            report.add(f"bundle target {target}", False, "unknown target, no staging rule")
            continue
        report.add(
            f"stages {target}",
            directory in bodies,
            f"copies from {directory}",
        )

    # The workspace root owns `target/`: staging from `src-tauri/target` is a
    # silent trap (the directory exists but is empty), so it is refused.
    wrong_target = "src-tauri/target" in bodies
    report.add(
        "staging reads the workspace target dir",
        not wrong_target,
        "target/release" if not wrong_target else "references src-tauri/target",
    )

    staging_guards = 0
    for job_name, step in steps:
        if job_name != "bundle" or not (step.get("name") or "").startswith("Stage"):
            continue
        script = step.get("run") or ""
        guarded = "[ ! -e" in script and "exit 1" in script
        staging_guards += 1 if guarded else 0
        report.add(
            f"{step.get('name')} fails loudly",
            guarded,
            "asserts every artefact exists",
        )

    staged = sorted(set(re.findall(r"Prism_\$\{VERSION\}_[\w.]*\.(?:msi|exe|AppImage|deb)", bodies)))
    published_globs = [
        pattern.rsplit("/", 1)[-1]
        for pattern in published
        if not pattern.endswith(("SHA256SUMS.txt", "THIRD-PARTY-LICENSES.md"))
    ]

    # A staged name is a shell glob (`Prism_${VERSION}_x64.msi`); instantiate it
    # with a plausible version and ask whether any published glob would pick the
    # resulting file up. That is exactly what `action-gh-release` does.
    samples = [name.replace("${VERSION}", "0.1.0") for name in staged]
    uncovered = [
        sample
        for sample in samples
        if not any(fnmatch.fnmatch(sample, pattern) for pattern in published_globs)
    ]
    report.add(
        "publishes every staged artefact",
        not uncovered,
        "ok" if not uncovered else f"uncovered={uncovered} published={published_globs}",
    )
    report.add(
        "publishes checksums",
        any("SHA256SUMS" in pattern for pattern in published),
        "SHA256SUMS.txt",
    )
    report.add(
        "publishes the licence inventory",
        any("THIRD-PARTY-LICENSES" in pattern for pattern in published),
        "THIRD-PARTY-LICENSES.md",
    )

    # The publish job must build its inputs before the release is created.
    publish_run = "\n".join(
        step.get("run") or "" for job, step in steps if job == "publish"
    )
    report.add(
        "release notes come from the changelog",
        "generate-changelog.mjs" in publish_run,
        "scripts/generate-changelog.mjs",
    )
    report.add(
        "checksums are regenerated",
        "sha256sum" in publish_run,
        "sha256sum over the downloaded artefacts",
    )


def check_changelog(report: Report, version: str) -> None:
    changelog = (ROOT / "CHANGELOG.md").read_text()
    report.add(
        "changelog has an Unreleased section",
        "## [Unreleased]" in changelog,
        "## [Unreleased]",
    )
    report.add(
        f"changelog has a {version} section",
        re.search(rf"^## \[{re.escape(version)}\]", changelog, re.MULTILINE) is not None,
        f"## [{version}]",
    )


def check_licence(report: Report) -> None:
    licence = ROOT / "LICENSE"
    text = licence.read_text() if licence.exists() else ""
    report.add("LICENSE is MIT", text.startswith("MIT License"), "MIT License")
    report.add(
        "LICENSE has a copyright line",
        re.search(r"Copyright \(c\) \d{4} \S", text) is not None,
        next((line for line in text.splitlines() if line.startswith("Copyright")), "?"),
    )
    tauri = json.loads((ROOT / "src-tauri" / "tauri.conf.json").read_text())
    report.add(
        "bundle metadata declares MIT",
        (tauri.get("bundle", {}).get("license") or "") == "MIT",
        f"bundle.license = {tauri.get('bundle', {}).get('license')}",
    )
    for manifest, field in (("package.json", "license"), ("Cargo.toml", "license")):
        content = (ROOT / manifest).read_text()
        pattern = rf'^\s*{field}\s*=\s*"([^"]+)"' if manifest.endswith(".toml") else rf'"{field}":\s*"([^"]+)"'
        match = re.search(pattern, content, re.MULTILINE)
        report.add(
            f"{manifest} declares MIT",
            match is not None and match.group(1) == "MIT",
            f"{field} = {match.group(1) if match else 'missing'}",
        )


def run_licence_audit(report: Report) -> None:
    try:
        result = subprocess.run(
            ["node", "scripts/license-audit.mjs", "--check"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=300,
        )
    except (OSError, subprocess.TimeoutExpired) as error:  # pragma: no cover
        report.add("licence audit", False, str(error))
        return
    output = (result.stdout.strip() or result.stderr.strip()).splitlines() or ["no output"]
    report.add("licence audit passes", result.returncode == 0, output[-1])


def main(argv: list[str]) -> int:
    args = [arg for arg in argv[1:] if not arg.startswith("--")]
    skip_licence = "--skip-licence" in argv

    package = json.loads((ROOT / "package.json").read_text())
    tag = args[0] if args else f"v{package['version']}"

    report = Report()
    workflow = load_workflow(report)
    version = check_versions(report, tag)
    if workflow:
        check_trigger(report, workflow)
        check_job_graph(report, workflow)
        check_action_pinning(report, workflow)
        check_scripts_exist(report, workflow)
        check_secrets(report, workflow)
        check_artefacts(report, workflow)
    check_changelog(report, version)
    check_licence(report)
    if not skip_licence:
        run_licence_audit(report)

    print(f"release candidate: {tag}\n")
    print(report.render())
    print()
    failed = sum(1 for _, ok, _ in report.rows if not ok)
    print(f"{len(report.rows) - failed}/{len(report.rows)} checks passed")
    return 1 if report.failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
