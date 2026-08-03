"""Verified-progress baseline observer for the neovim-glsl v0.4 decomposition.

The v0.4 spec makes the anti-fabrication rule bind on the decomposition input
itself: progress may enter the plan only as *verified observed state*, never as
recollection or assumption, and anything not verifiable stays unresolved.

Witnesses / enforces:
  - neovim_glsl.progress_record_verification  (observable_evidence)
  - neovim_glsl.progress_unverified_state      (unresolved)
  - neovim_glsl.no_recalled_progress_input
  - neovim_glsl.no_unverified_progress_claim
  - neovim_glsl.no_fabricated_progress

This observer records ONLY what it can check on the host it runs on:
git state and file presence. Claims that require a capability the host lacks
(for example: "the Rust candidate builds and runs" when cargo/rustc are absent)
are emitted as `unresolved`, with the reason, and are never counted as progress.

Pure standard library; the impure edges (file existence, tool presence, git)
are injected so the logic is deterministically testable.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
from pathlib import Path
from typing import Callable


def observe_git_head(repo_root: str | Path) -> dict:
    """Return the observed git HEAD, or an unresolved marker on failure."""
    try:
        head = subprocess.check_output(
            ["git", "-C", str(repo_root), "rev-parse", "HEAD"],
            stderr=subprocess.DEVNULL,
        )
        return {"observed": True, "head": head.decode().strip()}
    except (subprocess.CalledProcessError, OSError):
        return {"observed": False, "reason": "git rev-parse HEAD failed"}


def observe_artifacts(
    repo_root: str | Path,
    artifacts: list[str],
    exists: Callable[[str], bool] = os.path.exists,
) -> dict:
    """Split declared artifacts into verified-present and unresolved-missing.

    A path counts as progress ONLY if it is actually present. A missing path is
    unresolved, never silently treated as done.
    """
    verified: list[str] = []
    unresolved: list[dict] = []
    for rel in artifacts:
        full = str(Path(repo_root) / rel)
        if exists(full):
            verified.append(rel)
        else:
            unresolved.append({"artifact": rel, "reason": "declared but absent"})
    return {"verified_present": verified, "unresolved_missing": unresolved}


def observe_build_capability(
    tools: list[str],
    which: Callable[[str], str | None] = shutil.which,
) -> dict:
    """A build/run claim is verifiable only if its toolchain is present.

    When a required tool is missing the build status is unresolved, so the
    baseline can never claim "it builds" on a host that cannot build it.
    """
    missing = [t for t in tools if which(t) is None]
    if missing:
        return {
            "build_runs": "unresolved",
            "reason": f"toolchain absent on this host: {', '.join(missing)}",
        }
    return {"build_runs": "verifiable", "toolchain_present": tools}


def collect_declared_artifacts(units_path: str | Path) -> list[str]:
    raw = json.loads(Path(units_path).read_text(encoding="utf-8"))
    seen: list[str] = []
    for unit in raw["units"]:
        for ev in (unit.get("acceptance") or {}).get("evidence", []):
            if ev not in seen:
                seen.append(ev)
    return seen


def build_baseline(
    repo_root: str | Path,
    units_path: str | Path,
    build_tools: list[str],
    exists: Callable[[str], bool] = os.path.exists,
    which: Callable[[str], str | None] = shutil.which,
) -> dict:
    artifacts = collect_declared_artifacts(units_path)
    return {
        "schema": "neovim-glsl.progress-baseline/v0.4",
        "basis": "verified_observed_state",
        "disclaimer": "recollection and assumption are not progress; only "
        "host-observable evidence is recorded, everything else is unresolved",
        "git": observe_git_head(repo_root),
        "artifacts": observe_artifacts(repo_root, artifacts, exists=exists),
        "build_capability": observe_build_capability(build_tools, which=which),
    }


# The build tools the Mac-stage Rust candidate needs in order to be *verified*
# as building/running on the host that observes it.
MAC_CANDIDATE_BUILD_TOOLS = ["cargo", "rustc"]


if __name__ == "__main__":
    import sys

    here = Path(__file__).resolve().parent
    repo_root = sys.argv[1] if len(sys.argv) > 1 else str(here.parent)
    units = here / "work-units.json"
    baseline = build_baseline(repo_root, units, MAC_CANDIDATE_BUILD_TOOLS)
    print(json.dumps(baseline, indent=2, ensure_ascii=False))
