import os
import subprocess
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tests/recorder_transparency.lua"
NVIM = "/opt/homebrew/bin/nvim"


class ApiRecorderTransparencyTests(unittest.TestCase):
    def run_case(self, name):
        env = {
            **os.environ,
            "PATH": "/opt/homebrew/bin:" + os.environ.get("PATH", ""),
            "RECORDER_TEST_CASE": name,
        }
        result = subprocess.run(
            [NVIM, "--headless", "-u", "NONE", "-i", "NONE", "-n", "-l", str(SCRIPT)],
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_preserves_return_value(self):
        self.run_case("return_value")

    def test_preserves_multiple_return_values(self):
        self.run_case("multiple_return_values")

    def test_propagates_errors(self):
        self.run_case("error_propagation")


if __name__ == "__main__":
    unittest.main()
