# OpenShell #2640: independent correlation and deny-lifetime review

No blocking defect found in the bounded review below. This is an evidence-only downstream publication, not a competing implementation, formal approval, or merge-readiness claim.

## Pins and ownership

- Harry Turner / HarryMoss owns the implementation: `3fae097a5d4355eeff77746f3ea90e89228455e3`, branch `feat/2640-ocsf-trace-correlation/HarryMoss`.
- Series base: `f745aa4df4b1e63ca362cb4624d3952796a7c31c`.
- NVIDIA/OpenShell main read during review: `d789ec613b829044e4580f6cda7f5d4648a31285`.
- Adel Zaalouk / zanetworker owns #4288: `78621e8bb1fb9af4c91f9cf8dd5c3ebffc62b963`.
- rhuss owns the pending deployed default-level Collector/Tempo validation and contributed the schema/lifetime design.
- Reviewer: dot/ayo for kvnloo. Read root AGENTS.md, CONTRIBUTING.md, review-github-pr skill and sync-agent-infra maintenance map. Read-only independent review chunks covered correlation and network code; the primary reviewer executed every command reported below.

Harry's remote head and the #2640 discussion were rechecked after execution and before publication; head remained unchanged. Latest issue comment was Harry's handoff, `6074008613`. Fork issue and branch inventories were read before creating this evidence branch. The separate schema evidence branch remains unchanged.

## Executed commands and results

Native Linux x86_64, kernel 6.18.44; isolated Rust **1.95.0** (`59807616e`, 2026-04-14), matching rust-toolchain.toml. No source changes in the tested checkout. Dependencies resolved from committed Cargo.lock with `--locked`.

```sh
cd /workspace/openshell-review
export CARGO_HOME=/workspace/openshell-review-tools/cargo
export RUSTUP_HOME=/workspace/openshell-review-tools/rustup
export CARGO_TARGET_DIR=/workspace/openshell-review-tools/target
export CARGO_BUILD_JOBS=4
export PATH=/workspace/openshell-review-tools/cargo/bin:$PATH
cargo +1.95.0 test --locked -p openshell-ocsf -p openshell-otel
cargo +1.95.0 test --locked -p openshell-supervisor-network --lib deny -- --test-threads=2
```

1. OCSF/OTel: **220 passed, 0 failed**, two documentation examples ignored. OCSF: 169 unit, 25 integration, 2 compile-fail doctests. OTel: 18 unit, 6 correlation integration. Full output: `focused-tests.log`.
2. Network deny selection: **59 passed, 0 failed**, 1491 filtered. Full output: `network-tests.log`.

The six correlation integration tests cover valid sampled context, unsampled context, no active context, tracing without OTel, explicit override, and routed enrichment. Existing nine-builder trace serialization/typestate tests and both 1.1/1.3 downgrade contracts also pass.

All eight new network correlation tests passed:

- metadata rejections versus operational failures;
- default-level L7 relay deny;
- default-level middleware deny finding;
- response diagnostic findings only correlated as denies when delivery is denied;
- default-level L4 one-span root;
- DEBUG L4 connect/authorization parenting;
- allowed emissions do not create INFO deny spans;
- missing optional deny context leaves span attributes unset.

These tests call actual emission helpers and receive spans over loopback OTLP/gRPC. They do not drive every full asynchronous proxy path or a deployed sandbox.

## Source review and evidence limits

All locations below refer to Harry's exact pinned commit.

- [`openshell-otel/src/correlation.rs:15`](https://github.com/HarryMoss/OpenShell/blob/3fae097a5d4355eeff77746f3ea90e89228455e3/crates/openshell-otel/src/correlation.rs#L15): returns only valid, sampled current OTel context; does not extract workload headers.
- [`openshell-ocsf/src/trace.rs:30`](https://github.com/HarryMoss/OpenShell/blob/3fae097a5d4355eeff77746f3ea90e89228455e3/crates/openshell-ocsf/src/trace.rs#L30): explicit builder correlation takes precedence. Explicit ID validation is a documented trusted-caller responsibility.
- [`event_bridge.rs:37`](https://github.com/HarryMoss/OpenShell/blob/3fae097a5d4355eeff77746f3ea90e89228455e3/crates/openshell-ocsf/src/tracing_layers/event_bridge.rs#L37) and `:84`: both routes enrich before thread-local event installation and dispatch.
- [`telemetry.rs:63`](https://github.com/HarryMoss/OpenShell/blob/3fae097a5d4355eeff77746f3ea90e89228455e3/crates/openshell-supervisor-network/src/telemetry.rs#L63) through `:86`: INFO deny span encloses the actual synchronous emission, without an entered guard crossing await.
- [`proxy.rs:1141`](https://github.com/HarryMoss/OpenShell/blob/3fae097a5d4355eeff77746f3ea90e89228455e3/crates/openshell-supervisor-network/src/proxy.rs#L1141): connect handle survives the authorization worker return and reaches subsequent staged L4 deny emission.
- **Keep DEBUG parenting qualified:** [`proxy.rs:3026`](https://github.com/HarryMoss/OpenShell/blob/3fae097a5d4355eeff77746f3ea90e89228455e3/crates/openshell-supervisor-network/src/proxy.rs#L3026) drops connect before L7 relay. Later L7 denies can remain separate root traces even at DEBUG. This predates the patch; Harry's docs already qualify the live-parent condition. It is not a newly introduced regression. The DEBUG test proves staged L4 parenting, not a full CONNECT-to-L7 journey.

No deployed Podman/Collector/Tempo check, macOS/Windows/ARM64 execution, full workspace suite, or blanket CI/lint certification is claimed. No model calls, production mutations, service/security changes or competing implementation were made.

## Separately executed schema evidence

[Published schema receipt and replay material](https://github.com/kvnloo/OpenShell/blob/2e894d51aede75d92205b1659a7573c1b2cc3744/review-evidence/2640-4288/RESULTS.md).

The assigned schema reviewer ran Harry's OCSF suite (196 passed, 2 ignored), #4288's OCSF suite (198 passed, 2 ignored), three isolated downgrade checks, and one 54-case matrix (18 native no-ops, 24 successful downgrades, 12 kept-native cases). **These are separate executions, not unique regression totals. Harry's 196 overlaps this review's 220; do not sum them. The 54 cases are iterations of one test.** Harry already reported all-nine-builder validation; this is independent confirmation, not newly invented coverage.

Successful 1.1/1.3 output has no top-level trace or Trace profile and may retain both downgraded and independently supplied unmapped correlation. Failed conversion preserves the entire 1.8 event. The matrix overlays Harry's schemas and injects his serialized trace shape into #4288 builder JSON; **it is not a compiled full implementation rebase**. It uses the vendored validator, not the official external OCSF server, and retains #4288's documented container exception.

If #4288 lands first: retain Harry's Trace schemas and #4288's os/connection schema additions; replace the separate strip-list implementation with #4288's schema-driven downgrade; adapt Harry's two tests to `DowngradeOutcome::Downgraded`. Retain top-level/profile absence assertions without banning all unmapped trace IDs. A retained trace/profile is expected for `KeptNative` because the event still declares 1.8.

This reviewer stopped its own queued schema fixture before execution on reassignment. It is not published here and supplies no claimed schema result.

## Upstream delivery status

The user authorized one consolidated upstream review. The complete text is `UPSTREAM-COMMENT-DRAFT.md`.

```sh
gh api repos/NVIDIA/OpenShell/issues/2640/comments \
  -F body=@/workspace/shared/dot-openshell-2640-review/upstream-comment-draft.md \
  --jq '{id,html_url,body}'
```

GitHub rejected POST `/repos/NVIDIA/OpenShell/issues/2640/comments` with **HTTP 403: Resource not accessible by integration**. The JSON response is `upstream-post-denial.json`. No successful comment or comment URL exists. No retry through another identity or route was attempted.

This separately authorized downstream evidence publication does not post or relay the denied upstream comment. It adds only `review-evidence/2640-primary/` on a new fork branch, leaves the schema worker's branch unchanged, and opens no PR. rhuss's deployed validation remains pending.

## Replay and cleanup

Check out Harry's pinned revision, use Rust 1.95.0 and a disposable Cargo cache/target, and run the two commands above. No extra fixture or implementation patch is required; these are the author's existing tests.

The toolchain/cache was installed without modifying shell profiles. Disk was inventoried before installation (30G free) and after tests (25G free). After remote verification, only this reviewer's `/workspace/openshell-review-tools` disposable toolchain/download/build directory is eligible for cleanup. Pinned worktrees, published evidence, shared receipt files, and the unique paused Hermes regression remain preserved.
