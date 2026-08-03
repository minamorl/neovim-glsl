"""Tests for the v0.4 dependency-DAG validator.

Run: python3 -m unittest discover -s decomposition/tests -p 'test_*.py'
"""

import json
import tempfile
import unittest
from pathlib import Path

import sys

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import dependency_dag as dag  # noqa: E402


def _write(units):
    tmp = tempfile.NamedTemporaryFile(
        mode="w", suffix=".json", delete=False, encoding="utf-8"
    )
    json.dump({"units": units}, tmp)
    tmp.close()
    return tmp.name


_ACC = {"kind": "observable", "witness": "a snapshot renders", "evidence": []}


def _unit(uid, deps=(), acc=None, worktree="wt/x"):
    return {
        "id": uid,
        "title": uid,
        "owning_worktree": worktree,
        "depends_on": list(deps),
        "acceptance": acc if acc is not None else dict(_ACC),
    }


class RealDecompositionTest(unittest.TestCase):
    def test_committed_work_units_are_a_valid_dag(self):
        real = Path(__file__).resolve().parent.parent / "work-units.json"
        order = dag.validate(real)
        # Every dependency precedes its dependent in the returned order.
        units = {u.id: u for u in dag.load_units(real)}
        position = {uid: i for i, uid in enumerate(order)}
        for unit in units.values():
            for dep in unit.depends_on:
                self.assertLess(position[dep], position[unit.id])
        self.assertEqual(set(order), set(units))


class AcyclicityTest(unittest.TestCase):
    def test_linear_chain_orders_dependencies_first(self):
        path = _write([_unit("a"), _unit("b", ["a"]), _unit("c", ["b"])])
        self.assertEqual(dag.validate(path), ["a", "b", "c"])

    def test_diamond_is_acyclic(self):
        path = _write(
            [_unit("a"), _unit("b", ["a"]), _unit("c", ["a"]), _unit("d", ["b", "c"])]
        )
        order = dag.validate(path)
        self.assertEqual(order[0], "a")
        self.assertEqual(order[-1], "d")

    def test_cycle_is_rejected(self):
        path = _write([_unit("a", ["b"]), _unit("b", ["a"])])
        with self.assertRaises(dag.DagError) as ctx:
            dag.validate(path)
        self.assertIn("cycle", str(ctx.exception))

    def test_self_dependency_is_rejected(self):
        path = _write([_unit("a", ["a"])])
        with self.assertRaises(dag.DagError):
            dag.validate(path)

    def test_unknown_dependency_is_rejected(self):
        path = _write([_unit("a", ["ghost"])])
        with self.assertRaises(dag.DagError) as ctx:
            dag.validate(path)
        self.assertIn("unknown", str(ctx.exception))


class AcceptanceTest(unittest.TestCase):
    def test_missing_observable_acceptance_is_rejected(self):
        bad = {"kind": "subjective", "witness": "looks great"}
        path = _write([_unit("a", acc=bad)])
        with self.assertRaises(dag.DagError):
            dag.validate(path)

    def test_empty_witness_is_rejected(self):
        bad = {"kind": "observable", "witness": "   "}
        path = _write([_unit("a", acc=bad)])
        with self.assertRaises(dag.DagError):
            dag.validate(path)

    def test_missing_worktree_owner_is_rejected(self):
        path = _write([_unit("a", worktree="")])
        with self.assertRaises(dag.DagError):
            dag.validate(path)


class DuplicateTest(unittest.TestCase):
    def test_duplicate_id_is_rejected(self):
        path = _write([_unit("a"), _unit("a")])
        with self.assertRaises(dag.DagError):
            dag.validate(path)


if __name__ == "__main__":
    unittest.main()
