#!/usr/bin/env python3
"""Check that local links in Markdown files point at files that exist.

Scans every *.md in the repo (excluding .git, target, node_modules), strips
fenced code blocks and inline code, then validates every relative link whose
target does not start with http://, https://, mailto:, tel: or #.

URL-decodes %XX sequences, ignores pure anchors (#section), and checks
directory targets for existence. Exits 1 and prints a report on failure.
"""

import os
import re
import sys
import urllib.parse

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SKIP_DIRS = {".git", "target", "node_modules", ".gradle", "build", "dist"}

FENCED_CODE_RE = re.compile(r"```.*?```", re.DOTALL)
INLINE_CODE_RE = re.compile(r"`[^`\n]*`")
LINK_RE = re.compile(r"\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
DEF_LINK_RE = re.compile(r"^\s*\[[^\]]+\]:\s*(\S+)", re.MULTILINE)
SKIP_PREFIXES = ("http://", "https://", "mailto:", "tel:", "ftp://", "#")


def iter_markdown(root):
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS]
        for name in filenames:
            if name.lower().endswith(".md"):
                yield os.path.join(dirpath, name)


def strip_code(text):
    text = FENCED_CODE_RE.sub("", text)
    return INLINE_CODE_RE.sub("", text)


def check_file(path):
    rel_base = os.path.dirname(path)
    with open(path, encoding="utf-8") as handle:
        text = handle.read()
    text = strip_code(text)
    targets = [m.group(1) for m in LINK_RE.finditer(text)]
    targets += [m.group(1) for m in DEF_LINK_RE.finditer(text)]
    problems = []
    for target in targets:
        if target.startswith(SKIP_PREFIXES) or target.startswith("<"):
            continue
        target_path = target.split("#", 1)[0].split("?", 1)[0]
        if not target_path:
            continue
        decoded = urllib.parse.unquote(target_path)
        candidate = os.path.normpath(os.path.join(rel_base, decoded))
        if not os.path.exists(candidate):
            problems.append((target, candidate))
    return problems


def main():
    failures = []
    checked = 0
    for md in iter_markdown(REPO_ROOT):
        checked += 1
        for target, candidate in check_file(md):
            rel_md = os.path.relpath(md, REPO_ROOT)
            failures.append(f"{rel_md}: missing {target!r} (resolved to {os.path.relpath(candidate, REPO_ROOT)})")
    if failures:
        print(f"docs link check FAILED — {len(failures)} broken link(s) in {checked} file(s):")
        for failure in sorted(failures):
            print(f"  {failure}")
        return 1
    print(f"docs link check OK — {checked} markdown file(s) scanned, no broken local links")
    return 0


if __name__ == "__main__":
    sys.exit(main())
