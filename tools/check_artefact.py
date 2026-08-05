# -*- coding: utf-8 -*-
"""Fail when the measured candidate artefact has been modified.

`evaluation/candidate-embed-opengl/` is the embed candidate that spec v0.6 did
not select. It stays as it was measured — but `host/` reaches eight of its files
through `#[path]`, so they are modules of the host crate, and `cargo fmt` walks
the module graph. Reformatting them is a *consequence of the module tree*, not a
choice, which is why telling a delegate "do not format that directory" does not
hold: the instruction cannot be obeyed while still formatting `host/`.

rustfmt's own `ignore` option would solve it and is nightly-only, so it cannot
be used here. What is left is to catch rather than prevent — and to say so,
because a guard that looks like a prohibition invites someone to trust the
prohibition.

Usage:
    python3 tools/check_artefact.py            # against the working tree
    python3 tools/check_artefact.py --staged   # against the index

A run that finds no artefact files fails: an empty check that passes is worse
than no check.
"""
import subprocess
import sys

ARTEFACT = "evaluation/candidate-embed-opengl/"

staged = "--staged" in sys.argv
argv = ["git", "diff", "--name-only"] + (["--cached"] if staged else []) + ["--", ARTEFACT]
changed = [line for line in subprocess.run(argv, capture_output=True, text=True).stdout.split("\n") if line]

tracked = subprocess.run(
    ["git", "ls-files", ARTEFACT], capture_output=True, text=True
).stdout.split("\n")
tracked = [line for line in tracked if line.endswith(".rs")]
if not tracked:
    print("FAIL: no artefact sources found — this check is not looking where it thinks")
    sys.exit(2)

if not changed:
    print("ok: the measured artefact is untouched (%d sources checked)" % len(tracked))
    sys.exit(0)

print("The measured artefact has been modified:")
for path in changed:
    print("  " + path)
print()
print("If this is `cargo fmt` reaching them through `#[path]`, restore them:")
print("    git checkout -- " + ARTEFACT)
print("If a renderer change is genuinely needed, do not fold it in silently —")
print("`open_question embed_candidate_disposition` has not decided this artefact's fate.")
sys.exit(1)
