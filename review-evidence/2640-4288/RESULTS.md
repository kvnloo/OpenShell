# OpenShell schema compatibility review

Result: no schema/downgrade blocker found in the bounded checks below. OMP remains blocked; this is unrelated native Rust execution.

## Pins and attribution

- Harry Turner / HarryMoss: `3fae097a5d4355eeff77746f3ea90e89228455e3`, `feat/2640-ocsf-trace-correlation/HarryMoss`; trace implementation parent `a5e05fb7030d7b827c915917a4b13f4d0583f5eb`, separate downgrade tip. Remote head rechecked unchanged after execution.
- Adel Zaalouk / zanetworker, NVIDIA/OpenShell #4288: `78621e8bb1fb9af4c91f9cf8dd5c3ebffc62b963`, rechecked unchanged after execution.
- Schemas: vendored OCSF **1.1.0**, **1.3.0**, **1.8.0** at the pinned revisions. No external validator server called.
- Executor: Linux x86_64, isolated Rust/Cargo **1.95.0**. Initial host PATH had neither cargo nor rustc; rustup minimal was installed under this evidence directory, without changing shell profiles. One shared Cargo/target cache served all three worktrees.

## Executed checks

For each command, working directory was the named worktree under `/workspace/scratch/openshell-schema`. Environment prefix:

```sh
CARGO_HOME=/workspace/scratch/openshell-schema/cargo \
RUSTUP_HOME=/workspace/scratch/openshell-schema/rustup \
RUSTUP_TOOLCHAIN=1.95.0 \
CARGO_TARGET_DIR=/workspace/scratch/openshell-schema/target \
/workspace/scratch/openshell-schema/cargo/bin/cargo
```

1. `harry`: `cargo test --locked -p openshell-ocsf` — **196 passed, 2 ignored doctests**, all test groups successful. Breakdown 169 unit + 3 emission + 6 JSONL + 7 profile + 9 roundtrip + 2 compile-fail doctests. Log: `harry-tests.log`.
2. `pr4288`: same command — **198 passed, 2 ignored doctests**. Breakdown 172 unit + 6 JSONL + 7 profile + 9 roundtrip + 4 compile-fail doctests. Log: `pr4288-tests.log`.
3. `review` at #4288 plus one isolated test file: `cargo test --locked -p openshell-ocsf --features test-support --test review_trace_downgrade -- --nocapture` — **3 passed**, actual exit 0. Log: `compatibility-tests.log`; preserved test source: `review_trace_downgrade.rs`.
4. `review` with Harry's exact trace schema files overlaid, #4288 production functions unchanged: `cargo test --locked -p openshell-ocsf --lib review_schema_overlay -- --nocapture` — **1 matrix test passed, 54 cases**, actual exit 0. Nine existing `sample_events` builders × trace absent/present × requested version 1.8.0/1.3.0/1.1.0. Log: `schema-overlay-tests.log`; test-only patch: `review-schema-test.patch`.

**Counting:** these are separate executions, not counts of unique regressions. The unchanged Harry OCSF suite overlaps the primary reviewer's reported OCSF/OTel run. Do not add 196 to that reviewer's 220 as though all tests were distinct. The 54 cases are iterations inside one Rust test, not 54 separately registered tests. Harry already reported an all-nine-builder schema check; this matrix is independent confirmation, not a claim to invent that coverage. The three isolated checks additionally preserve distinct pre-existing unmapped correlation and exercise atomic kept-native behavior with trace present.

## Observable results

- Successful 1.1.0 and 1.3.0 conversions have neither top-level `trace` nor `trace` in `metadata.profiles`. Metadata names the full exact target version.
- #4288 preserves the native trace ID at `unmapped.downgraded_attributes.trace.uid`. An independently supplied `unmapped.trace.uid` remains unchanged too. Both target records pass #4288's vendored-schema recursive validator. A global search requiring absence of every trace ID would be an incorrect assertion.
- Missing source endpoint for 1.1.0, or destination endpoint for 1.3.0, returns `KeptNative` with the original event byte-equivalent as JSON, including its trace/profile and metadata.version=1.8.0. This is not a successful downgraded record.
- The 54-case matrix has 18 native no-ops, 24 successful downgrades, and 12 correct kept-native cases: SSH, Process, and Detection sample events lack target-required `action_id`. Every result validates against the version it actually claims.
- Native 1.8.0 target is a no-op and preserves all correlation/unmapped data.
- Class-schema structural comparison found the nine classes' non-trace attributes identical between the two pins. Harry supplies missing trace attributes/profile declarations on seven classes (HTTP/API already have them), plus trace/span/service/key-value object dependencies and the Trace profile. See `schema-comparison.json`.
- A run before the schema overlay failed at `Undefined attribute 'event.trace' in OCSF 1.8.0` on Network Activity. This is an expected composition negative control, not a blocker in Harry's change: retain his schema files when rebasing. Log: `schema-without-overlay-negative.log`.

## Exact source locations

- Harry `crates/openshell-ocsf/src/format/downgrade.rs:12` and `:15`: separate trace strip lists; `:116` through tests at `:142` / `:147`: successful 1.3.0/1.1.0 top-level/profile assertions.
- Harry `crates/openshell-ocsf/src/trace.rs:89`: existing nine-builder serialization/schema/roundtrip checks; `:130`: each class's optional trace profile coverage; `:162`: nested trace object dependency coverage.
- #4288 `crates/openshell-ocsf/src/format/downgrade.rs:94`: commit rewritten event only on successful conversion; `:152`: retain supported profiles; `:162`: prune then check requirements; `:165`: full target version; `:177`: removed fields under unmapped.
- #4288 `crates/openshell-ocsf/src/validation/schema.rs:69`: recursive validator; `:117`: rejects undefined attributes; `:85`: existing top-level container exception.

## Integration guidance and limits

If #4288 lands first, Harry's separate strip-list commit is superseded by the schema-driven downgrade. Adapt the two tests to `DowngradeOutcome::Downgraded`; keep their top-level/profile assertions and allow preserved unmapped values. Retain Harry's trace schemas and the existing #4288 os/connection schema additions. On `KeptNative`, a retained top-level trace is expected because the record still declares 1.8.0.

The overlay matrix executes real #4288 Rust functions, not a Python port or mock validator. Its serialized trace shape is injected into #4288 builder JSON; it is **not** a compiled full rebase of both implementations. Harry's actual trace setter/serialization is separately covered by his unchanged native suite. This work does not test OTel extraction, L4/L7 lifetimes, live sandbox deployment, Collector/Tempo export, or the official OCSF server. Those remain with the other reviewer/maintainer. The validator retains #4288's documented container exception and is not a general complete OCSF semantic validator.

Harness setup failures were corrected before success: the first isolated test used the module name `ctx` instead of `ctx::ctx` (compile error), and initial Python schema-copy commands hit read-only uv default paths; explicit scoped cache and installed Python resolved that. These attempts are not product failures or passes.

At original execution, no upstream posts, implementation fix, commit, push, merge, production mutation, or second framework had occurred. Original Harry/#4288 worktrees remain clean; the review worktree contains only isolated tests and the documented schema overlay.

## Downstream evidence publication

Published only as an evidence bundle on `kvnloo/OpenShell`, branch `test/2640-schema-compatibility/kvnloo`, based on the exact #4288 head above. The commit adds files only under `review-evidence/2640-4288/`; no production source, schemas, or normal test discovery changes. Implementation ownership remains with HarryMoss and zanetworker. Consolidated upstream review posting remains with the primary reviewer. No new test run was performed for publication.

Ownership/access was rechecked against the fork's current branches/issues and the #2640 timeline before publication. The GitHub search endpoint returned HTTP 502; the direct fork issue listing succeeded and contained all 15 issues, with no matching evidence implementation, and the remote branch inventory had no 2640/4288/schema/OCSF/trace branch. The assigned schema review is evidence-only and does not replace either author's fix.

`REPLAY.md` records reconstruction using the original pins and fixtures. `SHA256SUMS` binds the published report, logs, comparison, and fixture bytes. This bundle contains synthetic fixture events and public build output, not private transcripts or credentials.
