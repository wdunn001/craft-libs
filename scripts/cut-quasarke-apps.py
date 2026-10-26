#!/usr/bin/env python3
"""Create/update the quasarke branch on local Craft app checkouts.

Applies pins.toml, retargets release.yml at wdunn001/craft-common-actions@quasarke,
commits, and pushes origin (GitHub). Does not touch Forgejo remotes.
"""
from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

ROOTS = [
    Path(r"C:\Users\willi\collab-craft"),
    Path(r"C:\Users\willi\source\gh-wordcraft"),
    Path(r"C:\Users\willi\source\gh-gridcraft"),
    Path(r"C:\Users\willi\source\gh-figcraft"),
    Path(r"C:\Users\willi\source\gh-cadcraft"),
    Path(r"C:\Users\willi\source\gh-deckcraft"),
    Path(r"C:\Users\willi\source\gh-designcraft"),
    Path(r"C:\Users\willi\source\gh-effectcraft"),
    Path(r"C:\Users\willi\source\gh-filmcraft"),
    Path(r"C:\Users\willi\source\gh-lightcraft"),
    Path(r"C:\Users\willi\source\gh-pdfcraft"),
    Path(r"C:\Users\willi\source\gh-photocraft"),
    Path(r"C:\Users\willi\source\gh-soundcraft"),
    Path(r"C:\Users\willi\source\gh-vectorcraft"),
    Path(r"C:\Users\willi\source\gh-craft-launcher"),
    Path(r"C:\Users\willi\source\lasercraft"),
    Path(r"C:\Users\willi\source\solidcraft-quasarke"),
]
PINS = Path(r"C:\Users\willi\source\gh-craft-libs-quasarke\pins.toml")
APPLY = Path(r"C:\Users\willi\source\gh-craft-libs-quasarke\scripts\apply-craft-pins.py")
OLD_USES = "uses: storytold/craft-common-actions/.github/workflows/release.yml@main"
NEW_USES = "uses: wdunn001/craft-common-actions/.github/workflows/release.yml@quasarke"
ENV = {**os.environ, "GIT_TERMINAL_PROMPT": "0"}


def run(repo: Path, args: list[str], check: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(args, cwd=repo, env=ENV, text=True, capture_output=True, check=check)


def git(repo: Path, *args: str, check: bool = True) -> str:
    result = run(repo, ["git", *args], check=check)
    if check and result.returncode != 0:
        raise RuntimeError(result.stderr)
    return (result.stdout or "") + (result.stderr or "")


def retarget_release(repo: Path) -> bool:
    path = repo / ".github" / "workflows" / "release.yml"
    if not path.exists():
        return False
    text = path.read_text(encoding="utf-8")
    new = text.replace(OLD_USES, NEW_USES)
    new = new.replace("branches: [release]", "branches: [release, quasarke]")
    if new == text:
        return False
    path.write_text(new, encoding="utf-8", newline="\n")
    return True


def main() -> int:
    failures = []
    for repo in ROOTS:
        name = repo.name
        print(f"== {name} ==", flush=True)
        if not (repo / ".git").exists() and not (repo / ".git").is_file():
            print("  skip: no git")
            continue
        remotes = git(repo, "remote")
        if "forgejo" in remotes.split():
            # never fetch that remote from the laptop
            pass
        git(repo, "checkout", "-B", "quasarke")
        # drop local-only path patches that must not ship
        cargo = repo / "Cargo.toml"
        if cargo.exists():
            raw = cargo.read_text(encoding="utf-8")
            if "path = \"../gh-craft-libs" in raw or "path = '../gh-craft-libs" in raw:
                git(repo, "checkout", "--", "Cargo.toml", check=False)
        run(repo, [sys.executable, str(APPLY), "--pins", str(PINS), "--root", str(repo)])
        retarget_release(repo)
        status = git(repo, "status", "--porcelain")
        if not status.strip():
            print("  no changes")
        else:
            run(
            repo,
            [
                "git",
                "add",
                "--",
                "Cargo.toml",
                "Cargo.lock",
                ".github/workflows/release.yml",
                "apps",
                "crates",
            ],
            check=False,
        )
            # unstage lock-only noise if apply did not change manifests? keep lock if toml changed
            commit = run(
                repo,
                [
                    "git",
                    "-c", "user.name=wdunn001",
                    "-c", "user.email=wdunn001@gmail.com",
                    "commit",
                    "-m",
                    "Point git crates at pins.toml and build releases with GitHub Actions on quasarke.",
                ],
                check=False,
            )
            if commit.returncode != 0:
                print(commit.stdout)
                print(commit.stderr)
                if "nothing to commit" not in (commit.stdout + commit.stderr):
                    failures.append(name)
                    continue
        push = run(repo, ["git", "push", "-u", "origin", "quasarke"], check=False)
        print(push.stdout)
        print(push.stderr)
        if push.returncode != 0:
            failures.append(name)
            continue
        print("  pushed")
    if failures:
        print("failed:", ", ".join(failures))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
