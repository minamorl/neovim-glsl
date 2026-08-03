#!/usr/bin/env python3
"""Read both halves back and check what is actually comparable between them.

Two reports of the same script over the same corpus invite a single number
("host is faster"), and that number would be wrong: the halves use different
matchers, and the plugin-owned half includes a fixed settle. So this script
separates three things and refuses to let them blur.

**Checked.** The candidate *counts* per keystroke must agree. Two independent
matchers filtering the same corpus with the same query should admit the same set,
and if they do not, one of them is wrong about what matches — a correctness fact
that no timing figure would have surfaced.

**Reported, not judged.** Crossings per keystroke, and the durations each half
measured of its own work.

**Refused.** A verdict. `open_question neovim_glsl.navigation_state_owner` is
open at spec v0.9 and this script does not close it.

Exit status is 0 only when every check holds.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent
HOST = ROOT / "out/host-owned.json"
PLUGIN = ROOT / "out/measurement.json"


def fail(problems, message):
    problems.append(message)
    print(f"FAIL {message}")


def main():
    if not HOST.exists() or not PLUGIN.exists():
        raise SystemExit(
            "both halves must have been run:\n"
            "  python3 probe.py --tree <dir> --script <script>\n"
            "  cargo run -- --picker-script <script> --picker-corpus out/corpus.txt "
            "--picker-report out/host-owned.json"
        )

    host = json.loads(HOST.read_text())
    plugin = json.loads(PLUGIN.read_text())
    problems = []

    # --- the two halves must have been asked the same question ---------------
    if host["script"] != plugin["script"]:
        fail(problems, f"different scripts: {host['script']!r} vs {plugin['script']!r}")
    if host["corpus_entries"] != plugin["corpus_entries"]:
        fail(
            problems,
            f"different corpus sizes: {host['corpus_entries']} vs {plugin['corpus_entries']}",
        )

    corpus_path = Path(plugin["corpus_written_to"])
    if corpus_path.exists():
        written = [line for line in corpus_path.read_text().split("\n") if line]
        if len(written) != plugin["corpus_entries"]:
            fail(
                problems,
                f"corpus file has {len(written)} lines but the report says "
                f"{plugin['corpus_entries']}",
            )
    else:
        fail(problems, f"corpus file is missing: {corpus_path}")

    # --- the checkable claim: both halves agree on what matches --------------
    host_steps = host["steps"]
    plugin_steps = plugin["steps"]
    if len(host_steps) != len(plugin_steps):
        fail(problems, f"different step counts: {len(host_steps)} vs {len(plugin_steps)}")
    else:
        for i, (h, p) in enumerate(zip(host_steps, plugin_steps)):
            if h["query_after"] != p["query_after"]:
                fail(
                    problems,
                    f"step {i}: queries diverged ({h['query_after']!r} vs {p['query_after']!r})",
                )
            if h["rows_after"] != p["rows_after"]:
                fail(
                    problems,
                    f"step {i} query {h['query_after']!r}: candidate counts disagree "
                    f"({h['rows_after']} host vs {p['rows_after']} plugin) — one matcher "
                    "is admitting something the other rejects",
                )
            if h["empty_after"] != p["empty_after"]:
                fail(problems, f"step {i}: one half says empty and the other does not")

    # --- both halves must have been reports about something ------------------
    if host["process_boundaries_crossed_per_keystroke"] != 0:
        fail(problems, "the host-owned half claims to cross a process boundary")
    if plugin["process_boundaries_crossed_per_keystroke"] < 1:
        fail(problems, "the plugin-owned half claims to cross nothing")
    for name, report in (("host", host), ("plugin", plugin)):
        if "nothing" not in report["decides"]:
            fail(problems, f"the {name}-owned report claims to decide something")

    # --- selection must have been exercised ---------------------------------
    rows = {s["selection_after"] for s in plugin_steps if s["selection_after"] is not None}
    if len(rows) < 2:
        fail(problems, f"the plugin-owned half never moved the selection (rows {sorted(rows)})")
    rows = {s["selection_after"] for s in host_steps if s["selection_after"] is not None}
    if len(rows) < 2:
        fail(problems, f"the host-owned half never moved the selection (rows {sorted(rows)})")

    # --- reported, not judged ------------------------------------------------
    print()
    print(f"script                {host['script']!r}")
    print(f"corpus                {host['corpus_entries']} candidates from {plugin['tree']}")
    print(f"agreed candidate sets {[h['rows_after'] for h in host_steps]}")
    print()
    print("crossings per keystroke")
    print(f"  host-owned          {host['process_boundaries_crossed_per_keystroke']}")
    print(f"  plugin-owned        {plugin['process_boundaries_crossed_per_keystroke']}")
    print()
    print("each half's measurement of its own work, ms (not comparable as algorithms)")
    print(f"  host state update   p50 {host['state_update_ms']['p50']}  max {host['state_update_ms']['max']}")
    print(f"  host rows build     p50 {host['rows_build_ms']['p50']}  max {host['rows_build_ms']['max']}")
    print(f"  plugin state fetch  p50 {plugin['state_extract_ms']['p50']}  max {plugin['state_extract_ms']['max']}")
    print()
    print("the plugin-owned arrangement does not hand over:")
    for item in plugin["not_handed_over"]:
        print(f"  - {item}")
    print()

    # --- ordering differs, and that is not a failure -------------------------
    differing = [
        (h["query_after"], h["selected_after"], p["selected_after"])
        for h, p in zip(host_steps, plugin_steps)
        if h["selected_after"] != p["selected_after"]
    ]
    if differing:
        print(
            f"the two matchers ranked the same sets differently at {len(differing)} of "
            f"{len(host_steps)} steps, which is expected: fzy and a span-penalised scan "
            "are different orderings of the same candidates, e.g."
        )
        for query, a, b in differing[:3]:
            print(f"  {query!r}: host chose {a}, plugin chose {b}")
        print()

    print("open_question neovim_glsl.navigation_state_owner remains open.")
    print("open_question neovim_glsl.navigation_input_routing remains open.")

    if problems:
        print(f"\n{len(problems)} check(s) failed")
        return 1
    print("\nall checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
