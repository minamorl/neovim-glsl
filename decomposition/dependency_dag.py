"""Dependency-DAG validator for the neovim-glsl v0.4 work decomposition.

Witnesses these pins from pins/domains/neovim-glsl.spec@0.4:
  - neovim_glsl.work_dependency_graph  (directed_acyclic)
  - neovim_glsl.work_dependency_explicit  (dependencies explicitly declared)
  - neovim_glsl.no_dependency_cycle
  - neovim_glsl.work_unit_acceptance  (observable per unit)
  - neovim_glsl.work_unit_worktree_owner  (single owning worktree per unit)

Pure standard library so it runs on the host as-is (no network, no deps).
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


class DagError(ValueError):
    """Raised when a work-unit graph violates a v0.4 decomposition pin."""


@dataclass(frozen=True)
class WorkUnit:
    id: str
    title: str
    owning_worktree: str
    depends_on: tuple[str, ...]
    acceptance_kind: str
    acceptance_witness: str


def load_units(path: str | Path) -> list[WorkUnit]:
    raw = json.loads(Path(path).read_text(encoding="utf-8"))
    units: list[WorkUnit] = []
    for entry in raw["units"]:
        acc = entry.get("acceptance") or {}
        units.append(
            WorkUnit(
                id=entry["id"],
                title=entry.get("title", ""),
                owning_worktree=entry.get("owning_worktree", ""),
                depends_on=tuple(entry.get("depends_on", [])),
                acceptance_kind=acc.get("kind", ""),
                acceptance_witness=acc.get("witness", ""),
            )
        )
    return units


def _index(units: Iterable[WorkUnit]) -> dict[str, WorkUnit]:
    index: dict[str, WorkUnit] = {}
    for unit in units:
        if unit.id in index:
            raise DagError(f"duplicate work-unit id: {unit.id}")
        index[unit.id] = unit
    return index


def check_explicit_dependencies(units: Iterable[WorkUnit]) -> None:
    """Every declared dependency must reference a known unit; no self-edges.

    Witnesses neovim_glsl.work_dependency_explicit.
    """
    index = _index(units)
    for unit in index.values():
        for dep in unit.depends_on:
            if dep == unit.id:
                raise DagError(f"unit {unit.id} declares a self-dependency")
            if dep not in index:
                raise DagError(
                    f"unit {unit.id} depends on unknown unit: {dep}"
                )


def check_observable_acceptance(units: Iterable[WorkUnit]) -> None:
    """Each unit must carry an observable acceptance witness.

    Witnesses neovim_glsl.work_unit_acceptance and forbids
    neovim_glsl.no_subjective_only_acceptance.
    """
    for unit in units:
        if unit.acceptance_kind != "observable":
            raise DagError(
                f"unit {unit.id} acceptance must be 'observable', "
                f"got {unit.acceptance_kind!r}"
            )
        if not unit.acceptance_witness.strip():
            raise DagError(
                f"unit {unit.id} has an empty (non-observable) acceptance witness"
            )


def check_single_worktree_owner(units: Iterable[WorkUnit]) -> None:
    """Each unit must declare a single owning worktree.

    Witnesses neovim_glsl.work_unit_worktree_owner. (Distinctness of worktrees
    across *concurrently active* units is a scheduling property enforced by the
    integration gate, not a static graph property; here we require presence.)
    """
    for unit in units:
        if not unit.owning_worktree.strip():
            raise DagError(f"unit {unit.id} declares no owning worktree")


def topological_order(units: Iterable[WorkUnit]) -> list[str]:
    """Return a topological order, or raise DagError on a cycle.

    Witnesses neovim_glsl.work_dependency_graph = directed_acyclic and
    forbids neovim_glsl.no_dependency_cycle. Kahn's algorithm.
    """
    index = _index(units)
    check_explicit_dependencies(index.values())

    indegree = {uid: 0 for uid in index}
    dependents: dict[str, list[str]] = {uid: [] for uid in index}
    for unit in index.values():
        for dep in unit.depends_on:
            indegree[unit.id] += 1
            dependents[dep].append(unit.id)

    # Deterministic order: process ready nodes sorted by id.
    ready = sorted(uid for uid, deg in indegree.items() if deg == 0)
    order: list[str] = []
    while ready:
        uid = ready.pop(0)
        order.append(uid)
        for child in dependents[uid]:
            indegree[child] -= 1
            if indegree[child] == 0:
                ready.append(child)
        ready.sort()

    if len(order) != len(index):
        stuck = sorted(uid for uid, deg in indegree.items() if deg > 0)
        raise DagError(f"dependency cycle involves: {', '.join(stuck)}")
    return order


def validate(path: str | Path) -> list[str]:
    """Run every v0.4 graph gate; return the integration (topological) order."""
    units = load_units(path)
    check_explicit_dependencies(units)
    check_observable_acceptance(units)
    check_single_worktree_owner(units)
    return topological_order(units)


if __name__ == "__main__":
    import sys

    target = sys.argv[1] if len(sys.argv) > 1 else str(
        Path(__file__).with_name("work-units.json")
    )
    order = validate(target)
    print("dependency DAG valid; integration order:")
    for step, uid in enumerate(order, 1):
        print(f"  {step}. {uid}")
