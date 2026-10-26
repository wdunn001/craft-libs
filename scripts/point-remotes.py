#!/usr/bin/env python3
"""Point every Craft checkout: origin = wdunn001 GitHub, upstream = parent.

Never fetches or pushes Forgejo. Set GIT_TERMINAL_PROMPT=0. GitHub is origin
so `git push origin quasarke` hits the fork, not storytold.
"""
from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

ENV = {**os.environ, "GIT_TERMINAL_PROMPT": "0"}

# name -> (local path, parent GitHub owner or None)
REPOS = [
    ("cadcraft", Path(r"C:\Users\willi\source\gh-cadcraft"), "storytold"),
    ("deckcraft", Path(r"C:\Users\willi\source\gh-deckcraft"), "storytold"),
    ("designcraft", Path(r"C:\Users\willi\source\gh-designcraft"), "storytold"),
    ("effectcraft", Path(r"C:\Users\willi\source\gh-effectcraft"), "storytold"),
    ("figcraft", Path(r"C:\Users\willi\source\gh-figcraft"), "storytold"),
    ("filmcraft", Path(r"C:\Users\willi\source\gh-filmcraft"), "storytold"),
    ("gridcraft", Path(r"C:\Users\willi\source\gh-gridcraft"), "storytold"),
    ("lightcraft", Path(r"C:\Users\willi\source\gh-lightcraft"), "storytold"),
    ("pdfcraft", Path(r"C:\Users\willi\source\gh-pdfcraft"), "storytold"),
    ("photocraft", Path(r"C:\Users\willi\source\gh-photocraft"), "storytold"),
    ("soundcraft", Path(r"C:\Users\willi\source\gh-soundcraft"), "storytold"),
    ("vectorcraft", Path(r"C:\Users\willi\source\gh-vectorcraft"), "storytold"),
    ("wordcraft", Path(r"C:\Users\willi\source\gh-wordcraft"), "storytold"),
    ("craft-launcher", Path(r"C:\Users\willi\source\gh-craft-launcher"), "storytold"),
    ("craft-libs", Path(r"C:\Users\willi\source\gh-craft-libs-quasarke"), "storytold"),
    ("craft-common-actions", Path(r"C:\Users\willi\source\gh-craft-common-actions"), "storytold"),
    ("collab-craft", Path(r"C:\Users\willi\collab-craft"), None),
    ("lasercraft", Path(r"C:\Users\willi\source\lasercraft"), None),
    ("solidcraft", Path(r"C:\Users\willi\source\solidcraft"), "tangency-labs"),
]


def git(repo: Path, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run(["git", *args], cwd=repo, env=ENV, text=True, capture_output=True)


def remotes(repo: Path) -> dict[str, str]:
    out = git(repo, "remote", "-v").stdout
    found: dict[str, str] = {}
    for line in out.splitlines():
        parts = line.split()
        if len(parts) >= 3 and parts[2] == "(push)":
            found[parts[0]] = parts[1]
    return found


def ensure_url(repo: Path, name: str, url: str, existing: dict[str, str]) -> None:
    if name in existing:
        if existing[name].rstrip("/") != url.rstrip("/") and existing[name].rstrip("/") != url.removesuffix(".git"):
            r = git(repo, "remote", "set-url", name, url)
            if r.returncode:
                print("  set-url failed", name, r.stderr.strip())
            else:
                print(f"  {name} -> {url}")
        return
    r = git(repo, "remote", "add", name, url)
    if r.returncode:
        print("  add failed", name, r.stderr.strip())
    else:
        print(f"  added {name} -> {url}")


def main() -> int:
    for name, path, parent in REPOS:
        print(f"== {name} ==", flush=True)
        if not path.exists():
            print("  missing checkout")
            continue
        existing = remotes(path)
        origin = f"https://github.com/wdunn001/{name}.git"
        ensure_url(path, "origin", origin, existing)
        existing = remotes(path)
        if parent:
            ensure_url(path, "upstream", f"https://github.com/{parent}/{name}.git", existing)
            existing = remotes(path)
        # fork was a wdunn001 alias while origin still pointed at storytold
        fork = existing.get("fork", "")
        if "github.com/wdunn001/" in fork:
            git(path, "remote", "remove", "fork")
            print("  removed fork (now origin)")
        print("  remotes:", ", ".join(f"{k}={v}" for k, v in sorted(remotes(path).items())))
    return 0


if __name__ == "__main__":
    sys.exit(main())
