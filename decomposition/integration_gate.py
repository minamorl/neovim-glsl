"""Integration gate for the neovim-glsl v0.4 work decomposition.

A work unit may be integrated ONLY when both preconditions hold:
  1. its own acceptance is green, and
  2. every declared dependency is already integrated.

Witnesses / enforces:
  - neovim_glsl.integration_gate                 (integration is gated)
  - neovim_glsl.integration_gate_acceptance      (green_unit_acceptance)
  - neovim_glsl.integration_gate_dependencies    (integrated_declared_dependencies)
  - neovim_glsl.no_integration_before_acceptance
  - neovim_glsl.no_integration_before_dependencies

The declared-dependency map is supplied by the caller (at integration time it
comes from the v0.4 dependency-DAG unit), so this gate is independent of any
particular graph representation and is deterministically testable in isolation.
"""

from __future__ import annotations

from dataclasses import dataclass, field


class GateError(ValueError):
    """Raised when the gate is asked about an unknown unit."""


@dataclass(frozen=True)
class UnitState:
    id: str
    acceptance_green: bool
    integrated: bool = False


@dataclass(frozen=True)
class GateDecision:
    id: str
    allowed: bool
    blockers: tuple[str, ...] = field(default_factory=tuple)


def may_integrate(
    unit_id: str,
    declared_deps: dict[str, list[str]],
    states: dict[str, UnitState],
) -> GateDecision:
    if unit_id not in states:
        raise GateError(f"no state for unit: {unit_id}")
    if unit_id not in declared_deps:
        raise GateError(f"no declared dependencies for unit: {unit_id}")

    blockers: list[str] = []

    # Precondition 1: own acceptance must be green.
    if not states[unit_id].acceptance_green:
        blockers.append("acceptance is not green")

    # Precondition 2: every declared dependency already integrated.
    for dep in declared_deps[unit_id]:
        if dep not in states:
            blockers.append(f"declared dependency has no state: {dep}")
        elif not states[dep].integrated:
            blockers.append(f"declared dependency not integrated: {dep}")

    return GateDecision(id=unit_id, allowed=not blockers, blockers=tuple(blockers))


def evaluate_all(
    declared_deps: dict[str, list[str]],
    states: dict[str, UnitState],
) -> dict[str, GateDecision]:
    return {
        uid: may_integrate(uid, declared_deps, states) for uid in declared_deps
    }


def ready_to_integrate(
    declared_deps: dict[str, list[str]],
    states: dict[str, UnitState],
) -> list[str]:
    """Units that may integrate now AND are not yet integrated, sorted by id."""
    decisions = evaluate_all(declared_deps, states)
    return sorted(
        uid
        for uid, d in decisions.items()
        if d.allowed and not states[uid].integrated
    )


if __name__ == "__main__":
    # Illustrative dry run: a depends on nothing, b depends on a.
    deps = {"a": [], "b": ["a"]}
    st = {
        "a": UnitState("a", acceptance_green=True, integrated=False),
        "b": UnitState("b", acceptance_green=True, integrated=False),
    }
    print("ready first:", ready_to_integrate(deps, st))
    st["a"] = UnitState("a", acceptance_green=True, integrated=True)
    print("ready after a integrated:", ready_to_integrate(deps, st))
