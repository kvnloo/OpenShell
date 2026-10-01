#!/usr/bin/env python3
"""Downstream-only OpenShell doctor result -> z0.evidence.v0 adapter."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from typing import Sequence

_SCHEMA = "z0.evidence.v0"
_SHA = re.compile(r"[0-9a-fA-F]{40}")
_DRIVERS = {"docker", "podman"}


def normalize_doctor_result(
    *,
    revision: str,
    driver: str,
    returncode: int | None,
    error: str | None = None,
) -> dict:
    if not isinstance(revision, str) or not _SHA.fullmatch(revision):
        raise ValueError("revision must be a full 40-character Git SHA")
    if driver not in _DRIVERS:
        raise ValueError("driver must be docker or podman")

    if returncode is None:
        outcome = "unknown"
        result = "unknown"
    elif returncode == 0:
        outcome = "pass"
        result = "pass"
    else:
        outcome = "fail"
        result = "fail"

    return {
        "schema": _SCHEMA,
        "producer": {
            "name": "openshell",
            "repository": "kvnloo/OpenShell",
            "revision": revision,
        },
        "subject": {
            "kind": "runtime-check",
            "id": f"doctor-check:{driver}",
        },
        "outcome": outcome,
        "evidence": [
            {
                "id": "doctor-exit",
                "kind": "process-exit",
                "result": result,
                "details": {
                    "driver": driver,
                    "returncode": returncode,
                    "error_type": error,
                },
            }
        ],
        "invariants": [
            {
                "name": "portable-evidence-excludes-command-output",
                "result": "pass",
                "evidence_refs": ["doctor-exit"],
            }
        ],
    }


def run_doctor(command: Sequence[str], *, revision: str, driver: str) -> dict:
    try:
        completed = subprocess.run(
            [*command, "doctor", "check", "--driver", driver],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=30,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        return normalize_doctor_result(
            revision=revision,
            driver=driver,
            returncode=None,
            error=type(exc).__name__,
        )

    return normalize_doctor_result(
        revision=revision,
        driver=driver,
        returncode=completed.returncode,
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--revision", required=True)
    parser.add_argument("--driver", choices=sorted(_DRIVERS), required=True)
    parser.add_argument("--binary", default="openshell")
    args = parser.parse_args()

    receipt = run_doctor([args.binary], revision=args.revision, driver=args.driver)
    json.dump(receipt, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
