"""Full-pipeline integration test for the v0.4 decomposition check.

Runs against the real committed work-units.json and the real repo tree, so it
witnesses the actual current state rather than a fixture.

Run: python3 -m unittest discover -s decomposition/tests -p 'test_*.py'
"""

import unittest
from pathlib import Path

import sys

_DECOMP = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(_DECOMP))

import check  # noqa: E402


REPO_ROOT = _DECOMP.parent
UNITS = _DECOMP / "work-units.json"


class RealPipelineTest(unittest.TestCase):
    def setUp(self):
        # which -> None keeps the toolchain probe deterministic regardless of host.
        self.report = check.run(REPO_ROOT, UNITS, which=lambda t: None)

    def test_dependency_order_is_a_valid_topological_order(self):
        order = self.report["dependency_order"]
        self.assertIn("mac-stage-candidate", order)
        # mac-stage-candidate has no deps, so it must come before its dependents.
        self.assertEqual(order[0], "mac-stage-candidate")

    def test_zeno_stage_is_never_green(self):
        # Its witness has not been produced (evidence: []), so it stays unverified.
        zeno = self.report["acceptance"]["zeno-evaluation"]
        self.assertFalse(zeno["green"])
        self.assertIn("unverified", zeno["reason"])

    def test_mac_stage_candidate_is_green_from_present_evidence(self):
        # Its evidence PNGs are committed in the repo, so it is observably green.
        mac = self.report["acceptance"]["mac-stage-candidate"]
        self.assertTrue(mac["green"], mac["reason"])

    def test_build_capability_unresolved_when_toolchain_absent(self):
        self.assertEqual(
            self.report["baseline"]["build_capability"]["build_runs"], "unresolved"
        )

    def test_zeno_gate_blocked_on_acceptance(self):
        gate = self.report["gate_decisions"]["zeno-evaluation"]
        self.assertFalse(gate["allowed"])
        self.assertIn("acceptance is not green", gate["blockers"])

    def test_baseline_basis_is_verified_observed_state(self):
        self.assertEqual(self.report["baseline"]["basis"], "verified_observed_state")


if __name__ == "__main__":
    unittest.main()
