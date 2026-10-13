# Go SDK dotted gateway validation for NVIDIA/OpenShell#4509

**Evidence-only / no production repair.** This directory contains an opt-in Go test artifact. Its `.txt` suffix keeps the normal SDK test suite unchanged. The test must be copied into a disposable checkout to run.

## Source and ownership

- Tested origin `main`: `90161fbd7d87b6c07b88183b801b30b4925d84d7` (verified with `git ls-remote` and local `git rev-parse HEAD`). This evidence commit has that revision as its parent and changes only this evidence directory.
- CloneOfAlex reported [issue #4509](https://github.com/NVIDIA/OpenShell/issues/4509). As checked on 2026-10-13, it is open, unassigned, labeled `state:triage-needed`, and has no linked repair or maintainer acceptance.
- Drew owns adjacent [PR #4106](https://github.com/NVIDIA/OpenShell/pull/4106) directory override/listing work at `3425468f521907133bb100c0aedc79c0326d55d8`; this diagnostic does not duplicate or modify it. Credit for the existing Go SDK remains with its original authors.
- OpenShell is absent from the canonical OSS Factory project registry. Fork evidence here does not grant upstream publication rights. Maintainer acceptance, contributor vouch/DCO, registry, and origin-action authorization remain separate holds.

## Toolchain and baseline

The native run used the official Linux/amd64 `go1.25.13.linux-amd64.tar.gz`. Its SHA-256 was checked against the [official Go release manifest](https://go.dev/dl/?mode=json&include=all):

```text
39042a078ea9ceebe3ecda4a7188f0f5b96e14a071d27923ba7f40b456e85ae3  go1.25.13.linux-amd64.tar.gz
```

It was extracted into a task-local directory; no system Go binary or settings changed. SDK dependencies were resolved through ordinary Go module fetching into a task-local cache. No Buf regeneration or Cargo run was needed. Before the diagnostic was added, from `sdk/go`:

```shell
GOTOOLCHAIN=local GOMAXPROCS=2 go test -p 2 ./openshell/v1/gateway
```

Result: **PASS** (`ok ... 0.008s`). The repository's `sdk/go/go.mod` requires Go 1.25.13.

## Portable opt-in reproduction

In a disposable checkout of this evidence branch, using a genuine Go 1.25.13 or newer toolchain:

```shell
GO_BIN=/absolute/path/to/go1.25.13/bin/go
"$GO_BIN" version
git clone --depth=2 --filter=blob:none --sparse --branch evidence/4509-go-dotted-gateway https://github.com/kvnloo/OpenShell.git openshell-4509-check
cd openshell-4509-check
git sparse-checkout set sdk/go evidence/4509-go-dotted-gateway
test "$(git rev-parse HEAD^)" = 90161fbd7d87b6c07b88183b801b30b4925d84d7
cp evidence/4509-go-dotted-gateway/zz_issue4509_compat_test.go.txt sdk/go/openshell/v1/gateway/zz_issue4509_compat_test.go
cd sdk/go
GOTOOLCHAIN=local GOMAXPROCS=2 "$GO_BIN" test -p 2 -run '^TestIssue4509' -count=1 ./openshell/v1/gateway
GOTOOLCHAIN=local GOMAXPROCS=2 "$GO_BIN" test -p 2 -run 'TestIssue4509GatewayCompatibility/plain-gateway|TestIssue4509TraversalControls' -count=1 ./openshell/v1/gateway
```

The first focused run is expected to exit 1. On the pinned SDK, `gw.example.com` and non-loopback IPv4 `192.0.2.10` each fail five public entry-point checks: explicit and active `LoadConfig`, explicit and active `NewClient`, and `ListGateways`. That is **10 failing subtests**. Named calls return `gateway: invalid gateway name: name must not contain dots`; active calls wrap that as `no active gateway`; listing omits each configured directory. The second command exits 0: a dot-free gateway loads, builds clients and lists, while `.`, `..`, `../escape`, `/absolute`, `foo/bar`, and `foo\bar` return `ErrInvalidGatewayName`.

The artifact SHA-256 is `c895d430aa558a06a9133218444c92bf6ad79567da6b1166d73279ca2d25bfe6`. The opt-in file intentionally uses the package's existing `setupGateway` helper; do not run it outside the canonical `gateway` test package.

## What this establishes

The [CLI derives non-loopback names from URL hosts](https://github.com/NVIDIA/OpenShell/blob/90161fbd7d87b6c07b88183b801b30b4925d84d7/crates/openshell-cli/src/commands/gateway.rs#L899-L915), and [Rust bootstrap accepts a single normal path component](https://github.com/NVIDIA/OpenShell/blob/90161fbd7d87b6c07b88183b801b30b4925d84d7/crates/openshell-bootstrap/src/paths.rs#L41-L49). The [Go validator rejects all dots](https://github.com/NVIDIA/OpenShell/blob/90161fbd7d87b6c07b88183b801b30b4925d84d7/sdk/go/openshell/v1/gateway/paths.go#L88-L110), including ordinary hostname dots. The [Go SDK README](https://github.com/NVIDIA/OpenShell/blob/90161fbd7d87b6c07b88183b801b30b4925d84d7/sdk/go/README.md) describes CLI configuration compatibility. Existing Go tests deliberately reject `foo.bar` and `config.json`, so this is a contract disagreement requiring maintainer judgment; traversal protection must remain.

The fixtures create actual temporary XDG config directories, `metadata.json` and `active_gateway`. They do **not** execute CLI registration, connect to a gateway, authenticate, establish full Rust naming parity, or test symlink containment. Loopback URLs derive the special `openshell` name, so the issue does not affect every unnamed URL registration.

An independent xhigh critic reviewed the final diagnostic and found it sound for this scoped native reproduction. The critic did not rerun the tests; the command results above are the validator's native execution. No production fix or upstream action is proposed by this receipt.
