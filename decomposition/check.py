"""End-to-end v0.4 work-decomposition check for neovim-glsl.

Wires the three units together in dependency order:
  1. progress_baseline  — observe verified state (never fabricate progress)
  2. dependency_dag     — validate the graph is acyclic with observable acceptance
  3. integration_gate   — decide what may integrate now

Acceptance is treated as green for a unit iff its acceptance is observable AND
it declares at least one evidence artifact AND every declared artifact is
actually present on disk. A unit whose witness has not been produced yet (for
example the Zeno stage, evidence: []) is therefore never green -- it stays
unverified, exactly as the spec requires.

Run: python3 decomposition/check.py [repo_root]
"""

from __future__ import annotations

import json
import os
import shutil
import sys
from pathlib import Path
from typing import Callable

_HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE))

import dependency_dag as dag  # noqa: E402
import integration_gate as ig  # noqa: E402
import progress_baseline as pb  # noqa: E402


def declared_deps(units_path: str | Path) -> dict[str, list[str]]:
    raw = json.loads(Path(units_path).read_text(encoding="utf-8"))
    return {u["id"]: list(u.get("depends_on", [])) for u in raw["units"]}


def acceptance_is_green(
    unit: dict,
    repo_root: str | Path,
    exists: Callable[[str], bool] = os.path.exists,
) -> tuple[bool, str]:
    acc = unit.get("acceptance") or {}
    if acc.get("kind") != "observable":
        return False, "acceptance is not observable"
    evidence = acc.get("evidence") or []
    if not evidence:
        return False, "no evidence artifact produced yet (unverified, never green)"
    missing = [e for e in evidence if not exists(str(Path(repo_root) / e))]
    if missing:
        return False, f"evidence missing on disk: {', '.join(missing)}"
    return True, "all declared evidence present"


def run(
    repo_root: str | Path,
    units_path: str | Path,
    exists: Callable[[str], bool] = os.path.exists,
    which: Callable[[str], str | None] = None,
) -> dict:
    raw = json.loads(Path(units_path).read_text(encoding="utf-8"))
    units = {u["id"]: u for u in raw["units"]}

    order = dag.validate(units_path)  # raises on any DAG violation
    deps = declared_deps(units_path)

    acceptance: dict[str, dict] = {}
    states: dict[str, ig.UnitState] = {}
    for uid, unit in units.items():
        green, reason = acceptance_is_green(unit, repo_root, exists=exists)
        acceptance[uid] = {"green": green, "reason": reason}
        # A unit is considered integrated for gate purposes once its acceptance
        # is green (a real pipeline would flip this after the merge lands).
        states[uid] = ig.UnitState(uid, acceptance_green=green, integrated=green)

    which_fn = which if which is not None else shutil.which
    baseline = pb.build_baseline(
        repo_root, units_path, pb.MAC_CANDIDATE_BUILD_TOOLS,
        exists=exists, which=which_fn,
    )

    return {
        "schema": "neovim-glsl.decomposition-check/v0.4",
        "dependency_order": order,
        "baseline": baseline,
        "acceptance": acceptance,
        "gate_decisions": {
            uid: {"allowed": d.allowed, "blockers": list(d.blockers)}
            for uid, d in ig.evaluate_all(deps, states).items()
        },
    }


if __name__ == "__main__":
    repo_root = sys.argv[1] if len(sys.argv) > 1 else str(_HERE.parent)
    report = run(repo_root, _HERE / "work-units.json")
    print(json.dumps(report, indent=2, ensure_ascii=False))
