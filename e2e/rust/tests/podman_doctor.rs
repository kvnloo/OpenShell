// SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

#![cfg(feature = "e2e-podman")]

use std::process::Stdio;

use openshell_e2e::harness::binary::openshell_cmd;
use openshell_e2e::harness::output::strip_ansi;

#[tokio::test]
async fn doctor_check_passes_against_harness_podman_service() {
    if std::env::var("OPENSHELL_E2E_DRIVER").as_deref() != Ok("podman") {
        eprintln!("Skipping Podman doctor test: e2e driver is not podman");
        return;
    }

    let socket = std::env::var("OPENSHELL_PODMAN_SOCKET")
        .expect("Podman e2e harness must export OPENSHELL_PODMAN_SOCKET");

    let mut cmd = openshell_cmd();
    cmd.args(["doctor", "check", "--driver", "podman"])
        .env("OPENSHELL_PODMAN_SOCKET", &socket)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = cmd.output().await.expect("spawn openshell doctor");
    let clean = strip_ansi(&format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ));

    assert_eq!(
        output.status.code().unwrap_or(-1),
        0,
        "doctor should reach the Podman API service exported by the harness:\n{clean}"
    );
    assert!(clean.contains("Podman"), "missing Podman result:\n{clean}");
    assert!(
        clean.contains("All checks passed"),
        "missing success summary:\n{clean}"
    );
    assert!(
        clean.contains(&socket),
        "doctor should report the exact OpenShell Podman socket:\n{clean}"
    );
}
