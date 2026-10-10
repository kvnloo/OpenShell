# Podman resource preflight review of PR #3012

This AI-authored, test-only review is based on [@elezar's PR #3012](https://github.com/NVIDIA/OpenShell/pull/3012) at `c26ca9cc1514af46b38b75c47cef2a50c074824c`. @elezar owns the typed-resource implementation; Drew's PR review clarified its CPU/memory quantity contract. These tests exercise the remaining `validate_sandbox_create` boundary. No production fix is included.

| Focused test | Result on owner head plus two test-only compilation repairs |
| --- | --- |
| Omitted limits and `500m` CPU / `512Mi` memory | PASS (1 test) |
| CPU `0.000001` rejected in preflight | Expected RED: validation returned `Ok(())` |
| Memory `16Ei` rejected in preflight | Expected RED: validation returned `Ok(())` |

Before the two test-only repairs, **owner head plus the new tests** failed to compile with five errors (`E0609`/`E0277`): existing `container.rs` tests still accessed fields on the newly returned `Result`. The two added `.unwrap()` calls repair only those valid-input test call sites; no assertion ran in that initial attempt. Afterward, the control passed and each negative test failed at its `expect_err`, rather than at an unrelated condition. Shared quantity validation accepts both values, but Podman spec construction rejects the resulting zero-microsecond CPU quota and overflowing `u64` memory byte count only after provisioning has begun.

To reproduce from this branch with Rust 1.97.1 installed through rustup and the existing lockfile:

```sh
cargo +1.97.1 test --locked --offline -j 2 -p openshell-driver-podman --lib driver::tests::validate_sandbox_create_accepts_representable_resource_limits -- --exact
cargo +1.97.1 test --locked --offline -j 2 -p openshell-driver-podman --lib driver::tests::validate_sandbox_create_rejects_cpu_below_podman_quota_resolution -- --exact
cargo +1.97.1 test --locked --offline -j 2 -p openshell-driver-podman --lib driver::tests::validate_sandbox_create_rejects_memory_overflow -- --exact
```

The recorded focused runs used official Rust **1.97.1**, `--locked --offline -j 2`, and an isolated Cargo target; the repository pins **1.95.0**. The results are intentionally RED and are neither a full/pinned-toolchain pass nor GPU, daemon, or live-container acceptance. The tests request no mounts or GPU and do not contact a Podman or Docker daemon.
