#!/usr/bin/env python3
"""Rewrite git crate targets in Cargo.toml from pins.toml.

Usage:
  apply-craft-pins.py --pins pins.toml --root /path/to/app
  apply-craft-pins.py --pins-url https://raw.githubusercontent.com/wdunn001/craft-libs/quasarke/pins.toml --root .
"""
from __future__ import annotations

import argparse
import os
import re
import sys
import urllib.request
from pathlib import Path

try:
    import tomllib
except ImportError:  # pragma: no cover
    import tomli as tomllib  # type: ignore


INLINE = re.compile(
    r"(?P<crate>[\w-]+)\s*=\s*\{(?P<body>[^}]*)\}",
    re.MULTILINE,
)
GIT_FIELD = re.compile(r'git\s*=\s*"[^"]*"')
REV_FIELD = re.compile(r'rev\s*=\s*"[^"]*"')
BRANCH_FIELD = re.compile(r'branch\s*=\s*"[^"]*"')
PATCH_HEADER = re.compile(r'^\[patch\."https://github.com/[^"]+/(craft-libs|collab-craft)"\]\s*$', re.MULTILINE)
PATH_PATCH_TABLE = re.compile(
    r'^\[patch\."https://github.com/[^"]+/(?:craft-libs|collab-craft)"\]\n'
    r'(?:[^\n]*\n)*?(?=^\[|\Z)',
    re.MULTILINE,
)


def load_pins(path: Path | None, url: str | None) -> dict:
    if url:
        with urllib.request.urlopen(url, timeout=30) as resp:
            data = resp.read()
        return tomllib.loads(data.decode("utf-8") if isinstance(data, bytes) else data)
    if not path:
        raise SystemExit("pass --pins or --pins-url")
    with path.open("rb") as fh:
        return tomllib.load(fh)


def source_spec(pins: dict, key: str) -> dict:
    src = pins["sources"][key]
    spec = {"git": src["git"]}
    if src.get("rev"):
        spec["rev"] = src["rev"]
    elif src.get("branch"):
        spec["branch"] = src["branch"]
    else:
        raise SystemExit(f"source {key} needs rev or branch")
    return spec


def rewrite_body(body: str, spec: dict) -> str:
    if not GIT_FIELD.search(body):
        return body
    body = GIT_FIELD.sub(f'git = "{spec["git"]}"', body, count=1)
    if "rev" in spec:
        if REV_FIELD.search(body):
            body = REV_FIELD.sub(f'rev = "{spec["rev"]}"', body, count=1)
        else:
            body = BRANCH_FIELD.sub("", body)
            body = body.rstrip() + f', rev = "{spec["rev"]}"'
        body = BRANCH_FIELD.sub("", body)
    else:
        if BRANCH_FIELD.search(body):
            body = BRANCH_FIELD.sub(f'branch = "{spec["branch"]}"', body, count=1)
        else:
            body = REV_FIELD.sub(f'branch = "{spec["branch"]}"', body, count=1)
            if not BRANCH_FIELD.search(body):
                body = body.rstrip() + f', branch = "{spec["branch"]}"'
        body = REV_FIELD.sub("", body)
    body = re.sub(r",\s*,", ", ", body)
    body = re.sub(r",\s*$", "", body.strip())
    return body


def rewrite_toml(text: str, crate_map: dict[str, dict]) -> str:
    def inline(match: re.Match[str]) -> str:
        crate = match.group("crate")
        if crate not in crate_map:
            return match.group(0)
        body = rewrite_body(match.group("body"), crate_map[crate])
        return f"{crate} = {{{body}}}"

    text = INLINE.sub(inline, text)

    def patch_header(match: re.Match[str]) -> str:
        name = match.group(1)
        git = None
        for spec in crate_map.values():
            if spec["git"].rstrip("/").endswith("/" + name):
                git = spec["git"]
                break
        if not git:
            return match.group(0)
        return f'[patch."{git}"]'

    text = PATH_PATCH_TABLE.sub("", text)
    return PATCH_HEADER.sub(patch_header, text)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pins", type=Path)
    parser.add_argument("--pins-url")
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    pins = load_pins(args.pins, args.pins_url)
    crate_map = {
        crate: source_spec(pins, source)
        for crate, source in pins.get("crates", {}).items()
    }

    changed = []
    for toml in sorted(args.root.rglob("Cargo.toml")):
        rel = toml.relative_to(args.root)
        if any(part in ("target", "vendor", ".git") for part in rel.parts):
            continue
        original = toml.read_text(encoding="utf-8")
        rewritten = rewrite_toml(original, crate_map)
        if rewritten == original:
            continue
        if args.check:
            changed.append(str(rel))
            continue
        toml.write_text(rewritten, encoding="utf-8", newline="\n")
        changed.append(str(rel))

    if args.check:
        if changed:
            print("pin drift:", *changed, sep="\n")
            return 1
        print("pins match")
        return 0

    if changed:
        print("updated:")
        print("\n".join(changed))
    else:
        print("no Cargo.toml git pins needed changing")
    return 0


if __name__ == "__main__":
    sys.exit(main())
