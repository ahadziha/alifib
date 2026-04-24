#!/usr/bin/env python3
"""Mirror `<src>` into `<dest>` and write `<dest>/index.json`.

Used by both `just web-wasm` and the GitHub Pages deploy workflow so the
frontend fetches identical content in the local preview and the deployed
site.

Rules mirror `web/shared/src/lib.rs` exactly:
- Recursive walk; subdirectories are organisational.
- Each path segment (directory name or file stem) must match the alifib
  identifier rule `[A-Za-z_][A-Za-z0-9_]*`.  Anything else is skipped.
- A file's **stem** is its canonical name — globally unique across the
  tree, case-insensitively.  Duplicates exit non-zero.
- Index JSON is `{stem: relpath}` with forward slashes, sorted by key.
"""

import json
import os
import re
import shutil
import sys

IDENT_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


def valid_segment(name: str) -> bool:
    return bool(IDENT_RE.match(name))


def walk_ali(src_root: str):
    """Yield (stem, relpath) for every .ali file with ident-only segments."""
    for dirpath, dirnames, filenames in os.walk(src_root):
        # Skip invalid-segment directories entirely.  Mutating `dirnames`
        # in place is what `os.walk` uses to prune the descent.
        rel_dir = os.path.relpath(dirpath, src_root)
        if rel_dir != "." and not all(valid_segment(s) for s in rel_dir.split(os.sep)):
            dirnames[:] = []
            continue
        dirnames[:] = sorted(d for d in dirnames if valid_segment(d))
        for fn in sorted(filenames):
            if not fn.endswith(".ali"):
                continue
            stem = fn[:-4]
            if not valid_segment(stem):
                print(f"skipping {os.path.join(rel_dir, fn)} — stem not an identifier", file=sys.stderr)
                continue
            rel = os.path.join(rel_dir, fn) if rel_dir != "." else fn
            rel = rel.replace(os.sep, "/")
            yield stem, rel


def main(argv):
    if len(argv) != 3:
        print(f"usage: {argv[0]} <src-dir> <dest-dir>", file=sys.stderr)
        return 2

    src, dest = argv[1], argv[2]
    if not os.path.isdir(src):
        print(f"error: source directory {src!r} does not exist", file=sys.stderr)
        return 1

    # Detect duplicate stems case-insensitively — the server's scanner uses
    # the same rule, so disagreement between dev and CI is impossible.
    by_stem: dict[str, list[tuple[str, str]]] = {}
    for stem, rel in walk_ali(src):
        by_stem.setdefault(stem.lower(), []).append((stem, rel))

    duplicates = {k: v for k, v in by_stem.items() if len(v) > 1}
    if duplicates:
        print("error: duplicate example stems found (case-insensitive):", file=sys.stderr)
        for key, occurrences in sorted(duplicates.items()):
            paths = ", ".join(rel for _, rel in occurrences)
            print(f"  {key}: {paths}", file=sys.stderr)
        return 1

    # Fresh mirror — nuke and re-copy so removed upstream files disappear.
    if os.path.isdir(dest):
        shutil.rmtree(dest)
    os.makedirs(dest, exist_ok=True)

    manifest: dict[str, str] = {}
    for _, entries in by_stem.items():
        stem, rel = entries[0]
        src_path = os.path.join(src, rel.replace("/", os.sep))
        dest_path = os.path.join(dest, rel.replace("/", os.sep))
        os.makedirs(os.path.dirname(dest_path), exist_ok=True) if os.path.dirname(dest_path) else None
        shutil.copy2(src_path, dest_path)
        manifest[stem] = rel

    # Sort by stem so the committed/deployed manifest is byte-stable.
    manifest = dict(sorted(manifest.items()))
    with open(os.path.join(dest, "index.json"), "w") as f:
        json.dump(manifest, f, indent=2, sort_keys=True)
        f.write("\n")

    print(f"wrote {len(manifest)} examples → {dest}/ (manifest: index.json)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
