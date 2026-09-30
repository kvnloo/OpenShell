import importlib.util
import pathlib
import subprocess
import unittest
from unittest import mock


_PATH = pathlib.Path(__file__).with_name("doctor_evidence.py")


def _load():
    spec = importlib.util.spec_from_file_location("doctor_evidence", _PATH)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class DoctorEvidenceTest(unittest.TestCase):
    def setUp(self):
        self.module = _load()
        self.sha = "0123456789abcdef0123456789abcdef01234567"

    @mock.patch("subprocess.run")
    def test_success_is_pass_without_capturing_output(self, run):
        run.return_value = subprocess.CompletedProcess([], 0)
        receipt = self.module.run_doctor(
            ["openshell"], revision=self.sha, driver="podman"
        )
        self.assertEqual(receipt["outcome"], "pass")
        kwargs = run.call_args.kwargs
        self.assertIs(kwargs["stdout"], subprocess.DEVNULL)
        self.assertIs(kwargs["stderr"], subprocess.DEVNULL)

    @mock.patch("subprocess.run")
    def test_failure_is_fail(self, run):
        run.return_value = subprocess.CompletedProcess([], 7)
        receipt = self.module.run_doctor(
            ["openshell"], revision=self.sha, driver="docker"
        )
        self.assertEqual(receipt["outcome"], "fail")
        self.assertEqual(receipt["evidence"][0]["details"]["returncode"], 7)

    @mock.patch("subprocess.run")
    def test_unobserved_result_is_unknown(self, run):
        run.side_effect = subprocess.TimeoutExpired(["openshell"], 30)
        receipt = self.module.run_doctor(
            ["openshell"], revision=self.sha, driver="podman"
        )
        self.assertEqual(receipt["outcome"], "unknown")
        self.assertEqual(
            receipt["evidence"][0]["details"]["error_type"], "TimeoutExpired"
        )

    def test_receipt_details_exclude_runtime_output_and_secret_fields(self):
        receipt = self.module.normalize_doctor_result(
            revision=self.sha,
            driver="podman",
            returncode=0,
        )
        details = receipt["evidence"][0]["details"]
        self.assertEqual(
            set(details),
            {"driver", "returncode", "error_type"},
        )
        self.assertNotIn("stdout", details)
        self.assertNotIn("stderr", details)
        self.assertNotIn("command", details)
        self.assertNotIn("environment", details)

    def test_requires_exact_revision(self):
        with self.assertRaises(ValueError):
            self.module.normalize_doctor_result(
                revision="main", driver="podman", returncode=0
            )


if __name__ == "__main__":
    unittest.main()
