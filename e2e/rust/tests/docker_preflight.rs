// SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Doctor Docker preflight e2e tests.
//!
//! These tests verify that `openshell doctor check` reports actionable guidance
//! when Docker is not available.
//!
//! The tests do NOT require a running gateway or Docker — they intentionally
//! point `DOCKER_HOST` at a non-existent socket to simulate Docker being
//! unavailable.

use std::process::Stdio;
use std::time::Instant;
use std::fs;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use openshell_e2e::harness::binary::openshell_cmd;
use openshell_e2e::harness::output::strip_ansi;

/// Run `openshell <args>` in an isolated environment where Docker is
/// guaranteed to be unreachable.
///
/// Sets `DOCKER_HOST` to a non-existent socket so the preflight check
/// fails immediately regardless of the host's Docker configuration.
async fn run_without_docker(args: &[&str]) -> (String, i32, std::time::Duration) {
    let tmpdir = tempfile::tempdir().expect("create isolated config dir");
    let bin_dir = tmpdir.path().join("bin");
    fs::create_dir(&bin_dir).expect("create fake bin dir");
    let fake_docker = bin_dir.join("docker");
    fs::write(
        &fake_docker,
        "#!/bin/sh\n\
         echo 'Cannot connect to Docker daemon. Check DOCKER_HOST and run docker info.' >&2\n\
         exit 1\n",
    )
    .expect("write fake docker");
    #[cfg(unix)]
    fs::set_permissions(&fake_docker, fs::Permissions::from_mode(0o755))
        .expect("chmod fake docker");

    let start = Instant::now();

    let mut cmd = openshell_cmd();
    cmd.args(args)
        .env("XDG_CONFIG_HOME", tmpdir.path())
        .env("HOME", tmpdir.path())
        .env("PATH", &bin_dir)
        .env("DOCKER_HOST", "unix:///tmp/openshell-e2e-nonexistent.sock")
        .env_remove("OPENSHELL_GATEWAY")
        .env_remove("OPENSHELL_GATEWAY_ENDPOINT")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = cmd.output().await.expect("spawn openshell");
    let elapsed = start.elapsed();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{stdout}{stderr}");
    let code = output.status.code().unwrap_or(-1);
    (combined, code, elapsed)
}

// -------------------------------------------------------------------
// doctor check: validates system prerequisites
// -------------------------------------------------------------------

/// `openshell doctor check` with Docker unavailable should fail fast
/// and report the Docker check as FAILED.
#[tokio::test]
async fn doctor_check_fails_without_docker() {
    let (output, code, elapsed) = run_without_docker(&["doctor", "check"]).await;

    assert_ne!(
        code, 0,
        "doctor check should fail when Docker is unavailable, output:\n{output}"
    );

    assert!(
        elapsed.as_secs() < 10,
        "doctor check should complete quickly (took {}s)",
        elapsed.as_secs()
    );

    let clean = strip_ansi(&output);
    assert!(
        clean.contains("FAILED"),
        "doctor check should report Docker as FAILED:\n{clean}"
    );
}

/// `openshell doctor check` output should include the check label
/// so the user knows what was tested.
#[tokio::test]
async fn doctor_check_output_shows_docker_label() {
    let (output, _, _) = run_without_docker(&["doctor", "check"]).await;
    let clean = strip_ansi(&output);

    assert!(
        clean.contains("Docker"),
        "doctor check output should include 'Docker' label:\n{clean}"
    );
}

/// `openshell doctor check` with Docker unavailable should include
/// actionable guidance in the error output.
#[tokio::test]
async fn doctor_check_error_includes_guidance() {
    let (output, code, _) = run_without_docker(&["doctor", "check"]).await;

    assert_ne!(code, 0);
    let clean = strip_ansi(&output);

    assert!(
        clean.contains("DOCKER_HOST"),
        "doctor check error should mention DOCKER_HOST:\n{clean}"
    );
    assert!(
        clean.contains("docker info"),
        "doctor check error should suggest 'docker info':\n{clean}"
    );
}

/// When Docker IS available, `openshell doctor check` should pass and
/// report the version.
///
/// This test only runs when Docker is actually reachable on the host
/// (i.e., it will pass in CI with Docker but be skipped locally if
/// Docker is not running). We detect this by checking if the default
/// socket exists.
#[tokio::test]
async fn doctor_check_passes_with_docker() {
    if !std::path::Path::new("/var/run/docker.sock").exists() {
        eprintln!("skipping: /var/run/docker.sock not found");
        return;
    }

    let tmpdir = tempfile::tempdir().expect("create isolated config dir");
    let mut cmd = openshell_cmd();
    cmd.args(["doctor", "check", "--driver", "docker"])
        .env("XDG_CONFIG_HOME", tmpdir.path())
        .env("HOME", tmpdir.path())
        .env_remove("OPENSHELL_GATEWAY")
        .env_remove("OPENSHELL_GATEWAY_ENDPOINT")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = cmd.output().await.expect("spawn openshell");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{stdout}{stderr}");
    let code = output.status.code().unwrap_or(-1);
    let clean = strip_ansi(&combined);

    assert_eq!(
        code, 0,
        "doctor check should pass when Docker is available, output:\n{clean}"
    );
    assert!(
        clean.contains("All checks passed"),
        "doctor check should report success:\n{clean}"
    );
    assert!(
        clean.contains("ok"),
        "doctor check should show 'ok' for Docker:\n{clean}"
    );
}

// -------------------------------------------------------------------
// doctor check: falls back to Podman when Docker is absent
// -------------------------------------------------------------------

/// Run `openshell <args>` where only a fake `podman` is on `PATH`,
/// guaranteeing Docker cannot be found so the Podman check runs instead.
async fn run_podman_only(args: &[&str], podman_ok: bool) -> (String, i32) {
    let tmpdir = tempfile::tempdir().expect("create isolated config dir");
    let bin_dir = tmpdir.path().join("bin");
    fs::create_dir(&bin_dir).expect("create fake bin dir");
    let fake_podman = bin_dir.join("podman");
    let script = if podman_ok {
        "#!/bin/sh\necho '5.0.0'\n"
    } else {
        "#!/bin/sh\necho 'Cannot connect to Podman socket.' >&2\nexit 1\n"
    };
    fs::write(&fake_podman, script).expect("write fake podman");
    #[cfg(unix)]
    fs::set_permissions(&fake_podman, fs::Permissions::from_mode(0o755))
        .expect("chmod fake podman");

    let mut cmd = openshell_cmd();
    cmd.args(args)
        .env("XDG_CONFIG_HOME", tmpdir.path())
        .env("HOME", tmpdir.path())
        .env("PATH", &bin_dir)
        .env_remove("OPENSHELL_GATEWAY")
        .env_remove("OPENSHELL_GATEWAY_ENDPOINT")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = cmd.output().await.expect("spawn openshell");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);
    (format!("{stdout}{stderr}"), code)
}

/// `openshell doctor check` should validate Podman, not Docker, when
/// Docker is entirely absent from `PATH`.
#[tokio::test]
async fn doctor_check_falls_back_to_podman_label() {
    let (output, _) = run_podman_only(&["doctor", "check"], true).await;
    let clean = strip_ansi(&output);

    assert!(
        clean.contains("Podman"),
        "doctor check output should include 'Podman' label:\n{clean}"
    );
    assert!(
        !clean.contains("Docker"),
        "doctor check should not mention Docker when it is absent:\n{clean}"
    );
}

/// `openshell doctor check` with Podman unreachable should fail and
/// mention the Podman socket env var.
#[tokio::test]
async fn doctor_check_podman_failure_includes_guidance() {
    let (output, code) = run_podman_only(&["doctor", "check"], false).await;

    assert_ne!(code, 0, "doctor check should fail:\n{output}");
    let clean = strip_ansi(&output);
    assert!(
        clean.contains("OPENSHELL_PODMAN_SOCKET"),
        "doctor check error should mention OPENSHELL_PODMAN_SOCKET:\n{clean}"
    );
}


#[cfg(unix)]
mod runtime_selection {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    async fn run_with_fake_runtimes(
        args: &[&str],
        docker_script: Option<&str>,
        podman_script: Option<&str>,
        podman_socket: Option<&str>,
    ) -> (String, i32, std::time::Duration) {
        let tmpdir = tempfile::tempdir().expect("create isolated config dir");
        let bin_dir = tmpdir.path().join("bin");
        fs::create_dir(&bin_dir).expect("create fake bin dir");

        for (name, script) in [("docker", docker_script), ("podman", podman_script)] {
            let Some(script) = script else {
                continue;
            };
            let path = bin_dir.join(name);
            fs::write(&path, script).expect("write fake runtime");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                .expect("chmod fake runtime");
        }

        let start = Instant::now();
        let mut cmd = openshell_cmd();
        cmd.args(args)
            .env("XDG_CONFIG_HOME", tmpdir.path())
            .env("HOME", tmpdir.path())
            .env("PATH", &bin_dir)
            .env_remove("OPENSHELL_GATEWAY")
            .env_remove("OPENSHELL_GATEWAY_ENDPOINT")
            .env_remove("OPENSHELL_PODMAN_SOCKET")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(socket) = podman_socket {
            cmd.env("OPENSHELL_PODMAN_SOCKET", socket);
        }

        let output = cmd.output().await.expect("spawn openshell");
        let elapsed = start.elapsed();
        let clean = strip_ansi(&format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
        (clean, output.status.code().unwrap_or(-1), elapsed)
    }

    #[tokio::test]
    async fn doctor_check_mixed_runtime_health_uses_ready_candidate() {
        let docker = "#!/bin/sh\necho 'Docker daemon unavailable' >&2\nexit 1\n";
        let podman = "#!/bin/sh\necho '5.8.4'\nexit 0\n";
        let (output, code, _) =
            run_with_fake_runtimes(&["doctor", "check"], Some(docker), Some(podman), None).await;

        assert_eq!(code, 0, "healthy Podman should satisfy default doctor:\n{output}");
        assert!(output.contains("Docker"), "missing Docker result:\n{output}");
        assert!(output.contains("Podman"), "missing Podman result:\n{output}");
        assert!(output.contains("FAILED"), "missing failed Docker result:\n{output}");
        assert!(
            output.contains("At least one supported container runtime is ready"),
            "missing mixed-health summary:\n{output}"
        );
    }

    #[tokio::test]
    async fn doctor_check_driver_override_is_authoritative() {
        let docker = "#!/bin/sh\necho 'Docker daemon unavailable' >&2\nexit 1\n";
        let podman = "#!/bin/sh\necho '5.8.4'\nexit 0\n";
        let (output, code, _) = run_with_fake_runtimes(
            &["doctor", "check", "--driver", "docker"],
            Some(docker),
            Some(podman),
            None,
        )
        .await;

        assert_ne!(code, 0, "targeted Docker check must not be rescued by Podman:\n{output}");
        assert!(output.contains("Docker"), "missing Docker result:\n{output}");
        assert!(!output.contains("Podman"), "targeted check should ignore Podman:\n{output}");
    }

    #[tokio::test]
    async fn doctor_check_podman_socket_override_is_used() {
        let socket = "/tmp/openshell-test-podman.sock";
        let podman = "#!/bin/sh\nif [ \"$1\" != \"--url\" ] || [ \"$2\" != \"unix:///tmp/openshell-test-podman.sock\" ] || [ \"$3\" != \"version\" ]; then\n  echo 'missing expected --url socket override' >&2\n  exit 42\nfi\necho '5.8.4'\n";
        let (output, code, _) = run_with_fake_runtimes(
            &["doctor", "check", "--driver", "podman"],
            None,
            Some(podman),
            Some(socket),
        )
        .await;

        assert_eq!(code, 0, "explicit OpenShell Podman socket should pass:\n{output}");
        assert!(output.contains(socket), "doctor should report configured socket:\n{output}");
    }

    #[tokio::test]
    async fn doctor_check_runtime_probe_is_bounded() {
        let docker = "#!/bin/sh\nsleep 30\n";
        let (output, code, elapsed) = run_with_fake_runtimes(
            &["doctor", "check", "--driver", "docker"],
            Some(docker),
            None,
            None,
        )
        .await;

        assert_ne!(code, 0);
        assert!(output.contains("timed out after 5s"), "missing timeout diagnostic:\n{output}");
        assert!(
            elapsed.as_secs() < 8,
            "doctor probe should be bounded near five seconds (took {elapsed:?})"
        );
    }
}
