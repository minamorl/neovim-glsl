#!/usr/bin/env python3
"""Fail when TRACEABILITY.md has fallen behind the spec.

The table itself cannot be generated: its third column is a claim about *this
repository* — which file honours a pin — and no spec line contains that. So it is
written by hand, and it rotted the same way every hand transcription here has
rotted. At v0.9 it still declared `@0.7`, counted 14 domain pins, and carried none
of the five locus pins v0.8 added.

What *is* checkable is coverage: every pin, property and example id in the spec
must appear somewhere in the table, and the declared version must match the
spec's. That is what this checks. It does not check that the third column is true;
a wrong trace is a different failure from a missing one, and only the second can
be caught mechanically.

Usage:
    python3 tools/check_traceability.py pins/domains/neovim-glsl.spec
    python3 tools/check_traceability.py spec-mirror/neovim-glsl-0.9.lines

A mirror carries only live `quarantine` / `open_question` / `free` lines and has
no pin ids in it at all, so it is read together with a companion `.ids` manifest
that lists names and kinds — never statements, which stay in the spec so there is
no second place for them to drift.

A run that finds no ids fails. The first version of this script passed such a run,
which is the exact failure it exists to catch, wearing a green.
"""
import io
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
TABLE = ROOT / "TRACEABILITY.md"

if len(sys.argv) != 2:
    raise SystemExit(__doc__)

spec_path = pathlib.Path(sys.argv[1])
spec = io.open(spec_path, encoding="utf-8").read()
table = io.open(TABLE, encoding="utf-8").read()

from_mirror = "spec-mirror" in spec_path.parts

version = None
m = re.search(r"@meta version: ([0-9.]+)", spec)
if m:
    version = m.group(1)
if version is None:
    m = re.search(r"neovim-glsl-([0-9.]+)\.lines$", spec_path.name)
    if m:
        version = m.group(1)
if version is None:
    raise SystemExit("cannot determine spec version from the input")

problems = []

declared = re.search(r"neovim-glsl\.spec@([0-9.]+)", table)
if not declared:
    problems.append("TRACEABILITY.md does not declare which spec version it tracks")
elif declared.group(1) != version:
    problems.append(
        f"TRACEABILITY.md tracks @{declared.group(1)} but the spec is at {version}"
    )

ids = []
for line in spec.split("\n"):
    m = re.match(r"^(pin|property|example) ([A-Za-z0-9_.]+):", line)
    if m:
        ids.append((m.group(1), m.group(2)))

# A mirror has no pin lines. Read the companion manifest, which carries names and
# kinds only.
manifest = spec_path.with_suffix(".ids")
if not ids and manifest.exists():
    for line in io.open(manifest, encoding="utf-8").read().split("\n"):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split()
        if len(parts) >= 2 and parts[0] in ("pin", "property", "example"):
            ids.append((parts[0], parts[1]))
    print(f"ids read from {manifest.name}")

if not ids:
    print(
        f"no pin / property / example ids found in {spec_path}"
        + (f" or {manifest.name}" if not manifest.exists() else "")
    )
    print("a coverage check with nothing to check is not a pass")
    sys.exit(1)

missing = [(kind, name) for kind, name in ids if name not in table]

# A pin that is retired keeps its row, so a retired id is still expected to be
# present. Nothing here needs to distinguish them: absence is the failure either
# way.
if missing:
    problems.append(f"{len(missing)} spec id(s) absent from the table:")
    for kind, name in missing:
        problems.append(f"  {kind} {name}")

print(f"input   {spec_path} ({'mirror' if from_mirror else 'spec'})")
print(f"version {version}")
print(f"ids     {len(ids)} declared in the input, {len(ids) - len(missing)} present in the table")
if from_mirror:
    print(
        "note    a mirror carries only live lines, so ids retired in earlier\n"
        "        versions are not checked; run against the real spec for full coverage"
    )

if problems:
    print()
    for problem in problems:
        print(problem)
    print(f"\nFAIL")
    sys.exit(1)

print("\nok: every id in the input appears in the table, and the versions agree")
