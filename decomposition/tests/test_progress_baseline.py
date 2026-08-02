"""Tests for the verified-progress baseline observer.

Hermetic: file existence, tool presence, and the units file are all injected,
so the suite is green in this unit's own worktree without the integrated
work-units.json or a real toolchain.

Run: python3 -m unittest discover -s decomposition/tests -p 'test_*.py'
"""

import json
import tempfile
import unittest
from pathlib import Path

import sys

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import progress_baseline as pb  # noqa: E402


class ArtifactObservationTest(unittest.TestCase):
    def test_present_artifact_is_verified_missing_is_unresolved(self):
        present = {"/repo/a.png", "/repo/b.png"}
        result = pb.observe_artifacts(
            "/repo",
            ["a.png", "b.png", "ghost.png"],
            exists=lambda p: p in present,
        )
        self.assertEqual(sorted(result["verified_present"]), ["a.png", "b.png"])
        self.assertEqual(len(result["unresolved_missing"]), 1)
        self.assertEqual(result["unresolved_missing"][0]["artifact"], "ghost.png")

    def test_no_artifact_is_ever_verified_without_a_present_file(self):
        # A recollection-style claim (declared, but nothing on disk) must never
        # land in verified_present.
        result = pb.observe_artifacts(
            "/repo", ["remembered-but-absent"], exists=lambda p: False
        )
        self.assertEqual(result["verified_present"], [])
        self.assertEqual(len(result["unresolved_missing"]), 1)


class BuildCapabilityTest(unittest.TestCase):
    def test_missing_toolchain_leaves_build_unresolved(self):
        result = pb.observe_build_capability(
            ["cargo", "rustc"], which=lambda t: None
        )
        self.assertEqual(result["build_runs"], "unresolved")
        self.assertIn("cargo", result["reason"])

    def test_partial_toolchain_still_unresolved(self):
        result = pb.observe_build_capability(
            ["cargo", "rustc"],
            which=lambda t: "/bin/cargo" if t == "cargo" else None,
        )
        self.assertEqual(result["build_runs"], "unresolved")
        self.assertIn("rustc", result["reason"])

    def test_full_toolchain_is_verifiable(self):
        result = pb.observe_build_capability(
            ["cargo", "rustc"], which=lambda t: f"/bin/{t}"
        )
        self.assertEqual(result["build_runs"], "verifiable")


class DeclaredArtifactCollectionTest(unittest.TestCase):
    def test_collects_unique_evidence_paths(self):
        tmp = tempfile.NamedTemporaryFile(
            mode="w", suffix=".json", delete=False, encoding="utf-8"
        )
        json.dump(
            {
                "units": [
                    {"acceptance": {"evidence": ["x.png", "y.png"]}},
                    {"acceptance": {"evidence": ["y.png", "z.png"]}},
                    {"acceptance": {}},
                    {},
                ]
            },
            tmp,
        )
        tmp.close()
        self.assertEqual(
            pb.collect_declared_artifacts(tmp.name), ["x.png", "y.png", "z.png"]
        )


class BaselineShapeTest(unittest.TestCase):
    def test_baseline_records_basis_and_never_claims_missing_progress(self):
        tmp = tempfile.NamedTemporaryFile(
            mode="w", suffix=".json", delete=False, encoding="utf-8"
        )
        json.dump({"units": [{"acceptance": {"evidence": ["got.png", "no.png"]}}]}, tmp)
        tmp.close()
        baseline = pb.build_baseline(
            "/repo",
            tmp.name,
            ["cargo"],
            exists=lambda p: p.endswith("got.png"),
            which=lambda t: None,
        )
        self.assertEqual(baseline["basis"], "verified_observed_state")
        self.assertEqual(baseline["artifacts"]["verified_present"], ["got.png"])
        self.assertEqual(
            baseline["artifacts"]["unresolved_missing"][0]["artifact"], "no.png"
        )
        self.assertEqual(baseline["build_capability"]["build_runs"], "unresolved")


if __name__ == "__main__":
    unittest.main()
