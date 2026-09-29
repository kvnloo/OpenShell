// SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Doctor local-runtime preflight e2e tests.
//!
//! These tests isolate PATH and provide fake Docker/Podman executables so the
//! host's installed runtimes cannot change the result.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Stdio;
use std::time::Instant;

use openshell_e2e::harness::binary::openshell_cmd;
use openshell_e2e::harness::output::strip_ansi;

#[derive(Clone, Copy)]
enum FakeRuntime {
    Healthy(&'static str),
    Failed(&'static str),
}

fn install_fake_runtime(bin_dir: &std::path::Path, name: &str, runtime: FakeRuntime) {
    let path = bin_dir.join(name);
    let script = match runtime {
        FakeRuntime::Healthy(version) => {
            format!("#!/bin/sh\nprintf '%s\\n' '{version}'\nexit 0\n")
        }
        FakeRuntime::Failed(message) => {
            format!("#!/bin/sh\nprintf '%s\\n' '{message}' >&2\nexit 1\n")
        }
    };

    fs::write(&path, script).expect("write fake runtime");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod fake runtime");
}

async fn run_doctor(
    args: &[&str],
    docker: Option<FakeRuntime>,
    podman: Option<FakeRuntime>,
    podman_socket: Option<&str>,
) -> (String, i32, std::time::Duration) {
    let tmpdir = tempfile::tempdir().expect("create isolated config dir");
    let bin_dir = tmpdir.path().join("bin");
    fs::create_dir(&bin_dir).expect("create fake bin dir");

    if let Some(runtime) = docker {
        install_fake_runtime(&bin_dir, "docker", runtime);
    }
    if let Some(runtime) = podman {
        install_fake_runtime(&bin_dir, "podman", runtime);
    }

    let start = Instant::now();
    let mut cmd = openshell_cmd();
    cmd.args(args)
        .env("XDG_CONFIG_HOME", tmpdir.path())
        .env("HOME", tmpdir.path())
        .env("PATH", &bin_dir)
        .env("DOCKER_HOST", "unix:///tmp/openshell-e2e-nonexistent.sock")
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
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{stdout}{stderr}");
    let code = output.status.code().unwrap_or(-1);
    (combined, code, elapsed)
}

#[tokio::test]
async fn doctor_check_fails_when_only_docker_is_unreachable() {
    let (output, code, elapsed) = run_doctor(
        &["doctor", "check"],
        Some(FakeRuntime::Failed("Cannot connect to Docker daemon.")),
        None,
        None,
    )
    .await;

    assert_ne!(code, 0, "doctor check should fail:\n{output}");
    assert!(
        elapsed.as_secs() < 10,
        "doctor check should complete quickly (took {}s)",
        elapsed.as_secs()
    );

    let clean = strip_ansi(&output);
    assert!(clean.contains("Docker"), "missing Docker label:\n{clean}");
    assert!(clean.contains("FAILED"), "missing failed status:\n{clean}");
    assert!(clean.contains("DOCKER_HOST"), "missing guidance:\n{clean}");
    assert!(clean.contains("docker info"), "missing command guidance:\n{clean}");
}

#[tokio::test]
async fn doctor_check_passes_with_only_docker() {
    let (output, code, _) = run_doctor(
        &["doctor", "check"],
        Some(FakeRuntime::Healthy("27.3.1")),
        None,
        None,
    )
    .await;
    let clean = strip_ansi(&output);

    assert_eq!(code, 0, "doctor check should pass:\n{clean}");
    assert!(clean.contains("Docker"), "missing Docker label:\n{clean}");
    assert!(clean.contains("27.3.1"), "missing Docker version:\n{clean}");
    assert!(
        clean.contains("All checks passed"),
        "missing success summary:\n{clean}"
    );
}

#[tokio::test]
async fn doctor_check_passes_with_only_podman() {
    let (output, code, _) = run_doctor(
        &["doctor", "check"],
        None,
        Some(FakeRuntime::Healthy("5.8.2")),
        None,
    )
    .await;
    let clean = strip_ansi(&output);

    assert_eq!(code, 0, "doctor check should pass:\n{clean}");
    assert!(clean.contains("Podman"), "missing Podman label:\n{clean}");
    assert!(clean.contains("5.8.2"), "missing Podman version:\n{clean}");
    assert!(
        !clean.contains("Docker"),
        "absent Docker should not be reported:\n{clean}"
    );
}

#[tokio::test]
async fn doctor_check_default_passes_when_one_installed_runtime_is_healthy() {
    let (output, code, _) = run_doctor(
        &["doctor", "check"],
        Some(FakeRuntime::Failed("Docker daemon unavailable")),
        Some(FakeRuntime::Healthy("5.8.2")),
        None,
    )
    .await;
    let clean = strip_ansi(&output);

    assert_eq!(
        code, 0,
        "an unrelated broken runtime should not mask a healthy one:\n{clean}"
    );
    assert!(clean.contains("Docker"), "missing Docker result:\n{clean}");
    assert!(clean.contains("Podman"), "missing Podman result:\n{clean}");
    assert!(clean.contains("FAILED"), "missing failed Docker result:\n{clean}");
    assert!(
        clean.contains("At least one supported container runtime is ready"),
        "missing mixed-health summary:\n{clean}"
    );
}

#[tokio::test]
async fn doctor_check_driver_override_is_authoritative() {
    let (output, code, _) = run_doctor(
        &["doctor", "check", "--driver", "docker"],
        Some(FakeRuntime::Failed("Docker daemon unavailable")),
        Some(FakeRuntime::Healthy("5.8.2")),
        None,
    )
    .await;
    let clean = strip_ansi(&output);

    assert_ne!(
        code, 0,
        "targeted Docker check must not be rescued by Podman:\n{clean}"
    );
    assert!(clean.contains("Docker"), "missing Docker result:\n{clean}");
    assert!(
        !clean.contains("Podman"),
        "targeted check should ignore Podman:\n{clean}"
    );
}

#[tokio::test]
async fn doctor_check_podman_failure_names_openshell_socket() {
    let socket = "/tmp/openshell-test-podman.sock";
    let (output, code, _) = run_doctor(
        &["doctor", "check", "--driver", "podman"],
        None,
        Some(FakeRuntime::Failed("Podman service unavailable")),
        Some(socket),
    )
    .await;
    let clean = strip_ansi(&output);

    assert_ne!(code, 0);
    assert!(
        clean.contains("OPENSHELL_PODMAN_SOCKET"),
        "missing OpenShell socket guidance:\n{clean}"
    );
    assert!(
        clean.contains(socket),
        "missing configured socket path:\n{clean}"
    );
}

#[tokio::test]
async fn doctor_check_podman_socket_override_is_forwarded_to_cli() {
    let tmpdir = tempfile::tempdir().expect("create isolated config dir");
    let bin_dir = tmpdir.path().join("bin");
    fs::create_dir(&bin_dir).expect("create fake bin dir");
    let socket = "/tmp/openshell-test-podman.sock";
    let fake_podman = bin_dir.join("podman");
    fs::write(
        &fake_podman,
        format!(
            "#!/bin/sh\n\
             if [ \"$1\" != \"--url\" ] || [ \"$2\" != \"unix://{socket}\" ]; then\n\
               echo 'missing expected --url socket override' >&2\n\
               exit 42\n\
             fi\n\
             printf '%s\\n' '5.8.2'\n\
             exit 0\n"
        ),
    )
    .expect("write fake podman");
    fs::set_permissions(&fake_podman, fs::Permissions::from_mode(0o755))
        .expect("chmod fake podman");

    let mut cmd = openshell_cmd();
    cmd.args(["doctor", "check", "--driver", "podman"])
        .env("XDG_CONFIG_HOME", tmpdir.path())
        .env("HOME", tmpdir.path())
        .env("PATH", &bin_dir)
        .env("OPENSHELL_PODMAN_SOCKET", socket)
        .env_remove("OPENSHELL_GATEWAY")
        .env_remove("OPENSHELL_GATEWAY_ENDPOINT")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = cmd.output().await.expect("spawn openshell");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let clean = strip_ansi(&format!("{stdout}{stderr}"));

    assert_eq!(
        output.status.code().unwrap_or(-1),
        0,
        "Podman socket override should be forwarded as --url:\n{clean}"
    );
    assert!(clean.contains("5.8.2"), "missing Podman version:\n{clean}");
}
