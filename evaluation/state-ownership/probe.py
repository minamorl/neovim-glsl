#!/usr/bin/env python3
"""The other half: picker state owned by the plugin, and what the host must pay.

`open_question neovim_glsl.navigation_state_owner` asks who owns the picker's
query, candidate set and selection. spec v0.9 recorded that the human gate
answered 「わからない」 and pinned nothing, turning the question from one waiting
on a person into one waiting on an observation.

The host-owned arrangement is measured in Rust
(`evaluation/candidate-embed-opengl/src/picker_state.rs`, run with
`--picker-script`). This measures the plugin-owned arrangement against the
owner's real telescope: telescope keeps the state, and the host — which by spec
v0.8 is the only thing that can draw outside the grid — has to reach across the
process boundary for every row it draws.

It decides nothing. It reports what each keystroke costs in crossings, requests
and wall clock, and it reports the parts of a drawable row that this arrangement
does *not* hand over, because a comparison that only counted milliseconds would
miss those.

The Neovim plumbing is imported from `../protocol-surface/driver.py` rather than
copied, and the API recorder is left off: the recorder wraps every `nvim_*`
function, so a latency figure taken with it installed would be timing the
recorder.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import statistics
import sys
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parent
DRIVER = ROOT.parent / "protocol-surface/driver.py"


def load_driver():
    spec = importlib.util.spec_from_file_location("protocol_surface_driver", DRIVER)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


driver = load_driver()


# The keys the script may name, mapped to what Neovim should receive. The names
# match `picker_state::parse_script` in the Rust half so one string drives both.
KEYS = {
    "bs": "<BS>",
    "c-n": "<C-n>",
    "c-p": "<C-p>",
    "down": "<Down>",
    "up": "<Up>",
}


def parse_script(script: str):
    """Split a script into keystrokes. Unknown `<name>` is typed literally."""
    ops, i = [], 0
    while i < len(script):
        if script[i] == "<":
            end = script.find(">", i)
            if end != -1:
                name = script[i + 1 : end].lower()
                if name in KEYS:
                    ops.append((name, KEYS[name]))
                    i = end + 1
                    continue
        ops.append((script[i], script[i]))
        i += 1
    return ops


# What the host would have to ask for, per keystroke, to draw one frame of the
# picker: the query, where the selection is, and the rows themselves.
#
# This is written as Lua because the alternative — one `nvim_*` request per
# field — would measure a strawman: a real host would batch. Reading it in one
# `nvim_exec_lua` is the *cheapest* the plugin-owned arrangement can be, so the
# figures below are a lower bound on its cost rather than a worst case.
EXTRACT = r"""
local limit = ...
local ok, action_state = pcall(require, "telescope.actions.state")
if not ok then
  return vim.json.encode({ open = false, reason = "telescope.actions.state unavailable" })
end
local bufnrs = require("telescope.state").get_existing_prompt_bufnrs()
if #bufnrs == 0 then
  return vim.json.encode({ open = false, reason = "no prompt buffer" })
end
local picker = action_state.get_current_picker(bufnrs[1])
if not picker then
  return vim.json.encode({ open = false, reason = "no current picker" })
end

local manager = picker.manager
local total = manager and manager:num_results() or 0
local rows = {}
if manager then
  local index = 0
  for entry in manager:iter() do
    index = index + 1
    if index > limit then
      break
    end
    rows[#rows + 1] = {
      text = entry.ordinal or entry.value or tostring(entry),
      display = type(entry.display) == "string" and entry.display or nil,
    }
  end
end

return vim.json.encode({
  open = true,
  query = picker:_get_prompt(),
  total = total,
  selection_row = picker:get_selection_row(),
  selection = (function()
    local sel = picker:get_selection()
    if type(sel) == "table" then
      return sel.ordinal or sel.value or nil
    end
    return sel
  end)(),
  rows = rows,
})
"""


def summary(values):
    """Same shape as the Rust half's `Summary`, so the two reports read alike."""
    if not values:
        return None
    ordered = sorted(values)

    def pct(p):
        rank = max(1, int(-(-(p / 100.0 * len(ordered)) // 1)))
        return round(ordered[min(rank, len(ordered)) - 1], 4)

    return {
        "count": len(ordered),
        "min": round(ordered[0], 4),
        "mean": round(statistics.fmean(ordered), 4),
        "p50": pct(50),
        "p90": pct(90),
        "p95": pct(95),
        "p99": pct(99),
        "max": round(ordered[-1], 4),
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--script", default="src<c-n><c-n><c-p><bs>")
    parser.add_argument(
        "--tree",
        default=None,
        help="a real directory to pick from. Without it, protocol-surface's "
        "7-file scratch tree is used, which is too narrow for the selection to move.",
    )
    parser.add_argument("--visible-rows", type=int, default=12)
    parser.add_argument("--settle", type=float, default=0.35)
    parser.add_argument("--corpus-out", default=str(ROOT / "out/corpus.txt"))
    parser.add_argument("--output", default=str(ROOT / "out/measurement.json"))
    args = parser.parse_args()

    if args.tree:
        scratch = Path(args.tree).expanduser().resolve()
        if not scratch.is_dir():
            raise SystemExit(f"--tree is not a directory: {scratch}")
    else:
        scratch = ROOT / "out/scratch"
        driver.prepare_scratch(scratch)
    plugins = driver.plugin_paths()

    nvim = driver.Nvim()
    steps = []
    try:
        nvim.ui_attach()
        nvim.settle(0.25)
        # record_api=False: the recorder wraps every nvim_* function, and this
        # run measures durations rather than call counts.
        driver.install_and_load(nvim, scratch, plugins, record_api=False)
        nvim.settle(0.7)

        nvim.input("<Space>o")
        nvim.settle(1.1)

        # The corpus is whatever telescope offered at an empty query — not a
        # second list built by `find`, which would disagree with telescope's
        # ignore rules and make the two halves incomparable without saying so.
        opened = json.loads(nvim.exec_lua(EXTRACT, [100000]))
        if not opened.get("open"):
            raise SystemExit(f"the picker did not open: {opened.get('reason')}")
        corpus = [row["text"] for row in opened.get("rows", [])]
        if len(corpus) != opened.get("total"):
            raise SystemExit(
                "read {} candidates but the picker reports {}; the corpus would be "
                "a truncation".format(len(corpus), opened.get("total"))
            )
        if not corpus:
            raise SystemExit("the picker offered nothing; there is no corpus to filter")
        corpus_path = Path(args.corpus_out)
        corpus_path.parent.mkdir(parents=True, exist_ok=True)
        corpus_path.write_text("\n".join(corpus) + "\n")

        for label, keys in parse_script(args.script):
            started = time.monotonic()
            nvim.input(keys)
            nvim.settle(args.settle)
            input_ms = (time.monotonic() - started) * 1000.0

            started = time.monotonic()
            extracted = json.loads(nvim.exec_lua(EXTRACT, [args.visible_rows]))
            extract_ms = (time.monotonic() - started) * 1000.0

            steps.append(
                {
                    "op": label,
                    "keys_sent": keys,
                    "picker_open": extracted.get("open", False),
                    "query_after": extracted.get("query"),
                    "rows_after": extracted.get("total", 0),
                    "selection_after": extracted.get("selection_row"),
                    "selected_after": extracted.get("selection"),
                    "rows_returned": len(extracted.get("rows", []) or []),
                    "empty_after": extracted.get("total", 0) == 0,
                    "input_and_settle_ms": round(input_ms, 4),
                    "state_extract_ms": round(extract_ms, 4),
                    "reason": extracted.get("reason"),
                }
            )

        nvim.input("<Esc>")
        nvim.settle(0.3)
    finally:
        nvim.close()

    observed = [s for s in steps if s["picker_open"]]
    if not observed:
        raise SystemExit("the picker never opened; nothing was measured")
    rows_seen = {s["selection_after"] for s in observed if s["selection_after"] is not None}
    if len(rows_seen) < 2:
        raise SystemExit(
            "the selection never moved (rows seen: {}); widen the corpus or the "
            "script rather than filing this as a measurement".format(sorted(rows_seen))
        )
    report = {
        "schema": "neovim-glsl.picker-state-plugin-owned/v1",
        "arrangement": "plugin_owned_state",
        "records_for": [
            "open_question neovim_glsl.navigation_state_owner",
            "open_question neovim_glsl.navigation_input_routing",
        ],
        "decides": "nothing; both questions remain open at spec v0.9",
        "nvim_version_first_line": driver.nvim_version(),
        "plugins": plugins,
        # Absolute on purpose: this records where the observation was taken, not a
        # path to follow. Whether it is still there is a separate, stated fact.
        "tree": str(scratch),
        "tree_exists_now": scratch.is_dir(),
        "tree_is_real_repository": bool(args.tree),
        "corpus_entries": len(corpus),
        "corpus_source": "read from the opened picker at an empty query, so both "
        "halves filter exactly what telescope offered",
        # Relative to this file, so the report survives being merged out of the
        # worktree it was produced in.
        "corpus_written_to": str(corpus_path.relative_to(ROOT))
        if corpus_path.is_relative_to(ROOT)
        else str(corpus_path),
        "visible_rows": args.visible_rows,
        "script": args.script,
        "settle_seconds_per_keystroke": args.settle,
        # One crossing to deliver the key, one to fetch the state the host needs
        # in order to draw. The second is what the host-owned arrangement does
        # not have; it is the figure that belongs to the arrangement rather than
        # to any matcher.
        "process_boundaries_crossed_per_keystroke": 2,
        "rpc_requests_per_keystroke": 2,
        "keystrokes_observed_with_picker_open": len(observed),
        "state_extract_ms": summary([s["state_extract_ms"] for s in observed]),
        "steps": steps,
        "not_handed_over": [
            "match positions: telescope's entries carry the matched text, not the "
            "offsets the query hit, so a host drawing highlighted matches must "
            "re-derive them or ask for a different extraction",
            "scores: reachable per index via the entry manager, not in the batch above",
            "geometry: telescope's own window layout is meaningless to a surface "
            "outside the grid, so none of it transfers",
        ],
        "notes": [
            "The timings are fresh observations of this machine on this run.",
            "The API recorder from protocol-surface was deliberately not installed.",
            "The extraction is a single batched nvim_exec_lua, which is the cheapest "
            "this arrangement can be rather than the worst case.",
            "input_and_settle_ms includes a fixed settle and is therefore not a "
            "latency measurement; it is reported so the settle is visible, not hidden.",
        ],
    }

    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    print(output)


if __name__ == "__main__":
    main()
