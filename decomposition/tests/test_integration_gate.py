"""Tests for the v0.4 integration gate.

Run: python3 -m unittest discover -s decomposition/tests -p 'test_*.py'
"""

import unittest
from pathlib import Path

import sys

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import integration_gate as ig  # noqa: E402


def _states(spec):
    return {
        uid: ig.UnitState(uid, acceptance_green=g, integrated=i)
        for uid, (g, i) in spec.items()
    }


class AcceptanceGateTest(unittest.TestCase):
    def test_green_leaf_may_integrate(self):
        deps = {"a": []}
        d = ig.may_integrate("a", deps, _states({"a": (True, False)}))
        self.assertTrue(d.allowed)
        self.assertEqual(d.blockers, ())

    def test_red_acceptance_blocks_integration(self):
        # forbids no_integration_before_acceptance
        deps = {"a": []}
        d = ig.may_integrate("a", deps, _states({"a": (False, False)}))
        self.assertFalse(d.allowed)
        self.assertIn("acceptance is not green", d.blockers)


class DependencyGateTest(unittest.TestCase):
    def test_unintegrated_dependency_blocks(self):
        # forbids no_integration_before_dependencies
        deps = {"a": [], "b": ["a"]}
        states = _states({"a": (True, False), "b": (True, False)})
        d = ig.may_integrate("b", deps, states)
        self.assertFalse(d.allowed)
        self.assertIn("declared dependency not integrated: a", d.blockers)

    def test_integrated_dependency_unblocks(self):
        deps = {"a": [], "b": ["a"]}
        states = _states({"a": (True, True), "b": (True, False)})
        d = ig.may_integrate("b", deps, states)
        self.assertTrue(d.allowed)

    def test_both_preconditions_reported_together(self):
        deps = {"a": [], "b": ["a"]}
        states = _states({"a": (True, False), "b": (False, False)})
        d = ig.may_integrate("b", deps, states)
        self.assertFalse(d.allowed)
        self.assertIn("acceptance is not green", d.blockers)
        self.assertIn("declared dependency not integrated: a", d.blockers)


class ReadySetTest(unittest.TestCase):
    def test_ready_set_advances_as_dependencies_integrate(self):
        deps = {"a": [], "b": ["a"], "c": ["b"]}
        states = _states({"a": (True, False), "b": (True, False), "c": (True, False)})
        self.assertEqual(ig.ready_to_integrate(deps, states), ["a"])

        states["a"] = ig.UnitState("a", acceptance_green=True, integrated=True)
        self.assertEqual(ig.ready_to_integrate(deps, states), ["b"])

        states["b"] = ig.UnitState("b", acceptance_green=True, integrated=True)
        self.assertEqual(ig.ready_to_integrate(deps, states), ["c"])

    def test_already_integrated_units_drop_out_of_ready(self):
        deps = {"a": []}
        states = _states({"a": (True, True)})
        self.assertEqual(ig.ready_to_integrate(deps, states), [])


class GuardTest(unittest.TestCase):
    def test_unknown_unit_raises(self):
        with self.assertRaises(ig.GateError):
            ig.may_integrate("x", {}, {})


if __name__ == "__main__":
    unittest.main()
