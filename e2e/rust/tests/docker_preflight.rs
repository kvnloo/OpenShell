// SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

//! Deterministic doctor local-runtime preflight tests.
//!
//! PATH is isolated and populated with fake Docker/Podman executables so the
//! host's installed runtimes cannot change the result. Podman discovery tests
//! also own the first discovery socket, ahead of any host runtime socket.

use std::fs;
use std::io::{ErrorKind, Read as _, Write as _};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use openshell_e2e::harness::binary::openshell_cmd;
use openshell_e2e::harness::output::strip_ansi;

#[derive(Clone, Copy)]
enum FakeRuntime {
    Healthy(&'static str),
    Failed(&'static str),
    Hung,
}

struct PodmanSocket {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    server: Option<JoinHandle<()>>,
}

impl PodmanSocket {
    fn new(runtime_dir: &Path) -> Self {
        let socket_dir = runtime_dir.join("podman");
        fs::create_dir(&socket_dir).expect("create owned Podman socket dir");
        let path = socket_dir.join("podman.sock");
        let listener = UnixListener::bind(&path).expect("bind owned Podman socket");
        listener
            .set_nonblocking(true)
            .expect("make Podman listener nonblocking");

        let stop = Arc::new(AtomicBool::new(false));
        let server_stop = Arc::clone(&stop);
        let server = thread::spawn(move || {
            while !server_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        // Bound each connection as well as accept, so Drop can
                        // join even if a child exits without finishing _ping.
                        let timeout = Duration::from_millis(250);
                        stream
                            .set_read_timeout(Some(timeout))
                            .expect("set Podman ping read timeout");
                        stream
                            .set_write_timeout(Some(timeout))
                            .expect("set Podman ping write timeout");
                        let deadline = Instant::now() + timeout;
                        let mut request = [0_u8; 1024];
                        let mut total = 0;
                        while total < request.len()
                            && Instant::now() < deadline
                            && !server_stop.load(Ordering::Relaxed)
                        {
                            match stream.read(&mut request[total..]) {
                                Ok(0) | Err(_) => break,
                                Ok(read) => total += read,
                            }
                            if request[..total]
                                .windows(4)
                                .any(|bytes| bytes == b"\r\n\r\n")
                            {
                                if request[..total].starts_with(b"GET /_ping HTTP/1.1\r\n") {
                                    let _ = stream.write_all(
                                        b"HTTP/1.1 200 OK\r\nLibpod-Api-Version: 5.8.4\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK",
                                    );
                                }
                                break;
                            }
                        }
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) if error.kind() == ErrorKind::Interrupted => {}
                    Err(error) => panic!("accept owned Podman connection: {error}"),
                }
            }
        });

        Self {
            path,
            stop,
            server: Some(server),
        }
    }
}

impl Drop for PodmanSocket {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(server) = self.server.take() {
            let result = server.join();
            assert!(
                result.is_ok() || thread::panicking(),
                "owned Podman socket server panicked"
            );
        }
        let _ = fs::remove_file(&self.path);
    }
}

fn install_fake_runtime(bin_dir: &std::path::Path, name: &str, runtime: FakeRuntime) {
    let path = bin_dir.join(name);
    let script = match runtime {
        FakeRuntime::Healthy(version) if name == "podman" => format!(
            "#!/bin/sh\n\
             if [ \"$1\" = \"--version\" ]; then\n\
               printf '%s\\n' 'podman version {version}'\n\
               exit 0\n\
             fi\n\
             if [ \"$1\" = \"info\" ] && [ \"$2\" = \"--format\" ] && [ \"$3\" = \"json\" ]; then\n\
               printf '{{\"host\":{{\"serviceIsRemote\":false,\"remoteSocket\":{{\"path\":\"unix://%s/podman/podman.sock\"}}}}}}\\n' \"$XDG_RUNTIME_DIR\"\n\
               exit 0\n\
             fi\n\
             if [ \"$1\" = \"--url\" ] && [ \"$2\" = \"unix://$XDG_RUNTIME_DIR/podman/podman.sock\" ] && [ \"$3\" = \"version\" ] && [ \"$4\" = \"--format\" ] && [ \"$5\" = \"{{{{.Server.Version}}}}\" ]; then\n\
               printf '%s\\n' '{version}'\n\
               exit 0\n\
             fi\n\
             echo 'unexpected fake podman invocation' >&2\n\
             exit 42\n"
        ),
        FakeRuntime::Healthy(version) => {
            format!("#!/bin/sh\nprintf '%s\\n' '{version}'\nexit 0\n")
        }
        FakeRuntime::Failed(message) => {
            format!("#!/bin/sh\nprintf '%s\\n' '{message}' >&2\nexit 1\n")
        }
        FakeRuntime::Hung => "#!/bin/sh\n/bin/sleep 30\n".to_string(),
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

    // XDG_RUNTIME_DIR is checked before /run/user/<uid>; keep this listener
    // alive until the doctor child exits so a host socket cannot win.
    let _podman_socket =
        matches!(podman, Some(FakeRuntime::Healthy(_))).then(|| PodmanSocket::new(tmpdir.path()));

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
        .env("XDG_RUNTIME_DIR", tmpdir.path())
        .env("PATH", &bin_dir)
        .env("DOCKER_HOST", "unix:///tmp/openshell-e2e-nonexistent.sock")
        .env_remove("OPENSHELL_GATEWAY")
        .env_remove("OPENSHELL_GATEWAY_ENDPOINT")
        .env_remove("OPENSHELL_PODMAN_SOCKET")
        .env_remove("CONTAINER_HOST")
        .env_remove("CONTAINER_CONNECTION")
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
    let clean = strip_ansi(&output);

    assert_ne!(code, 0, "doctor check should fail:\n{clean}");
    assert!(
        elapsed.as_secs() < 10,
        "doctor check should complete quickly (took {}s)",
        elapsed.as_secs()
    );
    assert!(clean.contains("Docker"), "missing Docker label:\n{clean}");
    assert!(clean.contains("FAILED"), "missing failed status:\n{clean}");
    assert!(clean.contains("DOCKER_HOST"), "missing guidance:\n{clean}");
    assert!(
        clean.contains("docker info"),
        "missing command guidance:\n{clean}"
    );
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
        Some(FakeRuntime::Healthy("5.8.4")),
        None,
    )
    .await;
    let clean = strip_ansi(&output);

    assert_eq!(code, 0, "doctor check should pass:\n{clean}");
    assert!(clean.contains("Podman"), "missing Podman label:\n{clean}");
    assert!(clean.contains("5.8.4"), "missing Podman version:\n{clean}");
    assert!(
        !clean.contains("Docker"),
        "absent Docker should not be reported:\n{clean}"
    );
}

#[tokio::test]
async fn doctor_check_passes_when_one_installed_runtime_is_healthy() {
    let (output, code, _) = run_doctor(
        &["doctor", "check"],
        Some(FakeRuntime::Failed("Docker daemon unavailable")),
        Some(FakeRuntime::Healthy("5.8.4")),
        None,
    )
    .await;
    let clean = strip_ansi(&output);

    assert_eq!(
        code, 0,
        "healthy Podman should satisfy default doctor:\n{clean}"
    );
    assert!(clean.contains("Docker"), "missing Docker result:\n{clean}");
    assert!(clean.contains("Podman"), "missing Podman result:\n{clean}");
    assert!(
        clean.contains("FAILED"),
        "missing failed Docker result:\n{clean}"
    );
    assert!(
        clean.contains("At least one supported container runtime is ready"),
        "missing mixed-health summary:\n{clean}"
    );
    assert!(
        !clean.contains("All checks passed"),
        "mixed health must not claim all checks passed:\n{clean}"
    );
}

#[tokio::test]
async fn doctor_check_passes_when_both_runtimes_are_healthy() {
    let (output, code, _) = run_doctor(
        &["doctor", "check"],
        Some(FakeRuntime::Healthy("27.3.1")),
        Some(FakeRuntime::Healthy("5.8.4")),
        None,
    )
    .await;
    let clean = strip_ansi(&output);

    assert_eq!(code, 0, "both healthy runtimes should pass:\n{clean}");
    assert!(clean.contains("Docker"), "missing Docker result:\n{clean}");
    assert!(clean.contains("Podman"), "missing Podman result:\n{clean}");
    assert!(
        clean.contains("All checks passed"),
        "missing success summary:\n{clean}"
    );
}

#[tokio::test]
async fn doctor_check_fails_when_no_runtime_is_installed() {
    let (output, code, _) = run_doctor(&["doctor", "check"], None, None, None).await;
    let clean = strip_ansi(&output);

    assert_ne!(code, 0);
    assert!(
        clean.contains("no supported local container runtime found on PATH"),
        "missing no-runtime guidance:\n{clean}"
    );
}

#[tokio::test]
async fn doctor_check_fails_when_both_runtimes_are_unhealthy() {
    let (output, code, _) = run_doctor(
        &["doctor", "check"],
        Some(FakeRuntime::Failed("Docker daemon unavailable")),
        Some(FakeRuntime::Failed("Podman service unavailable")),
        None,
    )
    .await;
    let clean = strip_ansi(&output);

    assert_ne!(code, 0);
    assert!(clean.contains("Docker"), "missing Docker result:\n{clean}");
    assert!(clean.contains("Podman"), "missing Podman result:\n{clean}");
    assert!(
        clean.contains("no supported local container runtime is ready"),
        "missing aggregate failure:\n{clean}"
    );
}

#[tokio::test]
async fn doctor_check_driver_override_is_authoritative() {
    let (output, code, _) = run_doctor(
        &["doctor", "check", "--driver", "docker"],
        Some(FakeRuntime::Failed("Docker daemon unavailable")),
        Some(FakeRuntime::Healthy("5.8.4")),
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
async fn doctor_check_selected_podman_missing_is_not_rescued_by_docker() {
    let (output, code, _) = run_doctor(
        &["doctor", "check", "--driver", "podman"],
        Some(FakeRuntime::Healthy("27.3.1")),
        None,
        None,
    )
    .await;
    let clean = strip_ansi(&output);

    assert_ne!(code, 0);
    assert!(
        clean.contains("podman is not installed or not on PATH"),
        "missing selected-runtime error:\n{clean}"
    );
    assert!(
        !clean.contains("Docker"),
        "targeted check should ignore Docker:\n{clean}"
    );
}

#[tokio::test]
async fn doctor_check_rejects_empty_podman_socket() {
    let (output, code, _) = run_doctor(
        &["doctor", "check", "--driver", "podman"],
        None,
        Some(FakeRuntime::Healthy("5.8.4")),
        Some(""),
    )
    .await;
    let clean = strip_ansi(&output);

    assert_ne!(code, 0);
    assert!(
        clean.contains("OPENSHELL_PODMAN_SOCKET is set but empty"),
        "missing empty-socket diagnostic:\n{clean}"
    );
}

#[tokio::test]
async fn doctor_check_podman_without_override_uses_discovered_local_socket() {
    let (output, code, _) = run_doctor(
        &["doctor", "check", "--driver", "podman"],
        None,
        Some(FakeRuntime::Healthy("5.8.4")),
        None,
    )
    .await;
    let clean = strip_ansi(&output);

    assert_eq!(
        code, 0,
        "discovered local Podman socket should pass:\n{clean}"
    );
    assert!(
        clean.contains("5.8.4"),
        "missing Podman server version:\n{clean}"
    );
    assert!(
        clean.contains("/podman/podman.sock"),
        "doctor should report the discovered local socket:\n{clean}"
    );
}

#[tokio::test]
async fn doctor_check_unrelated_remote_cannot_rescue_failed_local_podman_service() {
    for remote in [
        "ssh://core@example.invalid/run/user/1000/podman.sock",
        "tcp://example.invalid:2375",
    ] {
        let tmpdir = tempfile::tempdir().expect("create isolated config dir");
        let bin_dir = tmpdir.path().join("bin");
        fs::create_dir(&bin_dir).expect("create fake bin dir");
        // Discovery accepts our _ping, but the selected local API service fails
        // its version probe. A healthy unrelated remote must not rescue it.
        let local_socket = PodmanSocket::new(tmpdir.path());
        let fake_podman = bin_dir.join("podman");
        fs::write(
            &fake_podman,
            format!(
                "#!/bin/sh\n\
                 if [ \"$1\" = \"--version\" ]; then\n\
                   printf '%s\\n' 'podman version 5.8.4'\n\
                   exit 0\n\
                 fi\n\
                 if [ \"$1\" = \"--url\" ]; then\n\
                   if [ \"$2\" = \"unix://$XDG_RUNTIME_DIR/podman/podman.sock\" ] && [ \"$3\" = \"version\" ] && [ \"$4\" = \"--format\" ] && [ \"$5\" = \"{{{{.Server.Version}}}}\" ]; then\n\
                     printf 'owned local Podman API service failed: podman --url %s version\\n' \"$2\" >&2\n\
                     exit 1\n\
                   fi\n\
                   if [ \"$2\" != \"$CONTAINER_HOST\" ]; then\n\
                     echo 'unexpected local Podman socket or version arguments' >&2\n\
                     exit 42\n\
                   fi\n\
                   shift 2\n\
                 fi\n\
                 if [ \"$1\" = \"info\" ] && [ \"$2\" = \"--format\" ] && [ \"$3\" = \"json\" ]; then\n\
                   printf '%s\\n' '{{\"host\":{{\"serviceIsRemote\":true}}}}'\n\
                   exit 0\n\
                 fi\n\
                 if [ \"$1\" = \"system\" ] && [ \"$2\" = \"connection\" ]; then\n\
                   printf '%s\\n' '[{{\"Name\":\"remote\",\"URI\":\"{remote}\",\"Default\":true,\"IsMachine\":false}}]'\n\
                   exit 0\n\
                 fi\n\
                 if [ \"$1\" = \"--remote\" ]; then\n\
                   shift\n\
                 fi\n\
                 if [ \"$1\" = \"version\" ] || [ \"$1\" = \"info\" ]; then\n\
                   printf '%s\\n' '99.0.0'\n\
                   exit 0\n\
                 fi\n\
                 echo 'unexpected fake podman invocation' >&2\n\
                 exit 42\n"
            ),
        )
        .expect("write fake podman");
        fs::set_permissions(&fake_podman, fs::Permissions::from_mode(0o755))
            .expect("chmod fake podman");

        // Prove that the generic CLI probe really would accept the remote.
        let remote_output = tokio::process::Command::new(&fake_podman)
            .args(["version", "--format", "{{.Server.Version}}"])
            .env("CONTAINER_HOST", remote)
            .output()
            .await
            .expect("probe fake remote Podman");
        assert!(remote_output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&remote_output.stdout).trim(),
            "99.0.0"
        );

        let mut cmd = openshell_cmd();
        cmd.args(["doctor", "check", "--driver", "podman"])
            .env("XDG_CONFIG_HOME", tmpdir.path())
            .env("HOME", tmpdir.path())
            .env("XDG_RUNTIME_DIR", tmpdir.path())
            .env("PATH", &bin_dir)
            .env("CONTAINER_HOST", remote)
            .env_remove("CONTAINER_CONNECTION")
            .env_remove("OPENSHELL_PODMAN_SOCKET")
            .env_remove("OPENSHELL_GATEWAY")
            .env_remove("OPENSHELL_GATEWAY_ENDPOINT")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = cmd.output().await.expect("spawn openshell");
        let clean = strip_ansi(&format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));

        assert_ne!(
            output.status.code().unwrap_or(-1),
            0,
            "unrelated Podman remote {remote} must not rescue a failed local service:\n{clean}"
        );
        assert!(clean.contains("FAILED"), "missing failed status:\n{clean}");
        assert!(
            clean.contains(&format!(
                "owned local Podman API service failed: podman --url unix://{} version",
                local_socket.path.display()
            )),
            "doctor must report the exact owned-socket version failure:\n{clean}"
        );
        assert!(
            clean.contains("Check the local Podman API service or Podman Machine socket; OpenShell requires a host-local Unix socket."),
            "missing local-endpoint guidance:\n{clean}"
        );
        assert!(
            !clean.contains("99.0.0"),
            "doctor must not report the unrelated remote's version:\n{clean}"
        );
    }
}

#[tokio::test]
async fn doctor_check_podman_socket_override_is_authoritative() {
    let tmpdir = tempfile::tempdir().expect("create isolated config dir");
    let bin_dir = tmpdir.path().join("bin");
    fs::create_dir(&bin_dir).expect("create fake bin dir");
    let socket = "/tmp/openshell-test-podman.sock";
    let fake_podman = bin_dir.join("podman");
    fs::write(
        &fake_podman,
        format!(
            "#!/bin/sh\n\
             if [ \"$1\" != \"--url\" ] || [ \"$2\" != \"unix://{socket}\" ] || [ \"$3\" != \"version\" ]; then\n\
               echo 'missing expected --url socket override' >&2\n\
               exit 42\n\
             fi\n\
             printf '%s\\n' '5.8.4'\n"
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
        .env(
            "CONTAINER_HOST",
            "ssh://wrong.example.invalid/run/podman/podman.sock",
        )
        .env("CONTAINER_CONNECTION", "wrong-connection")
        .env_remove("OPENSHELL_GATEWAY")
        .env_remove("OPENSHELL_GATEWAY_ENDPOINT")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = cmd.output().await.expect("spawn openshell");
    let clean = strip_ansi(&format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ));

    assert_eq!(
        output.status.code().unwrap_or(-1),
        0,
        "OpenShell socket override should win over Podman connection state:\n{clean}"
    );
    assert!(
        clean.contains(socket),
        "missing configured socket path:\n{clean}"
    );
}

#[tokio::test]
async fn doctor_check_runtime_probe_times_out() {
    let (output, code, elapsed) = run_doctor(
        &["doctor", "check", "--driver", "docker"],
        Some(FakeRuntime::Hung),
        None,
        None,
    )
    .await;
    let clean = strip_ansi(&output);

    assert_ne!(code, 0);
    assert!(
        clean.contains("timed out after 5s"),
        "missing timeout diagnostic:\n{clean}"
    );
    assert!(
        elapsed.as_secs() < 8,
        "single runtime timeout should be bounded near five seconds (took {elapsed:?})"
    );
}

#[tokio::test]
async fn doctor_check_default_probes_run_concurrently() {
    let (output, code, elapsed) = run_doctor(
        &["doctor", "check"],
        Some(FakeRuntime::Hung),
        Some(FakeRuntime::Hung),
        None,
    )
    .await;
    let clean = strip_ansi(&output);

    assert_ne!(code, 0);
    assert!(
        clean.matches("timed out after 5s").count() >= 2,
        "missing timeout results:\n{clean}"
    );
    assert!(
        elapsed.as_secs() < 8,
        "two five-second probes should run concurrently, not serially (took {elapsed:?})"
    );
}
