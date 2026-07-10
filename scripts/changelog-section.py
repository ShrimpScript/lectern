#!/usr/bin/env python3
"""Print one version's section from CHANGELOG.md — the body between its header and the next
version (or the link-reference block) — for use as a GitHub Release body.

Usage:
    scripts/changelog-section.py <version> [changelog-path]

<version> is "X.Y.Z" or "Unreleased". Exits non-zero if the version isn't found, so a
release script can fail loudly rather than publish an empty body.
"""
import re
import sys


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: changelog-section.py <version> [CHANGELOG.md]", file=sys.stderr)
        return 2
    version = sys.argv[1]
    path = sys.argv[2] if len(sys.argv) > 2 else "CHANGELOG.md"
    try:
        lines = open(path, encoding="utf-8").read().splitlines()
    except OSError as e:
        print(f"cannot read {path}: {e}", file=sys.stderr)
        return 1

    header = re.compile(r"^##\s+\[" + re.escape(version) + r"\]")
    start = next((i + 1 for i, ln in enumerate(lines) if header.match(ln)), None)
    if start is None:
        print(f"version {version} not found in {path}", file=sys.stderr)
        return 3

    body: list[str] = []
    for ln in lines[start:]:
        # Stop at the next version header or the link-reference block at the file's foot.
        if re.match(r"^##\s+", ln) or re.match(r"^\[[^\]]+\]:\s", ln):
            break
        body.append(ln)

    while body and not body[0].strip():
        body.pop(0)
    while body and not body[-1].strip():
        body.pop()

    print("\n".join(body))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
