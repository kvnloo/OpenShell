# Reconstruct the isolated checks

This is a recipe for the checks already recorded in RESULTS.md, not a new test run. Use disposable worktrees and isolated tool/cache directories; never apply the overlay to a user's active checkout.

1. Fetch HarryMoss/OpenShell commit `3fae097a5d4355eeff77746f3ea90e89228455e3` and NVIDIA/OpenShell commit `78621e8bb1fb9af4c91f9cf8dd5c3ebffc62b963`. Create separate detached worktrees `harry`, `pr4288`, and `review` (the latter also at #4288).
2. With Rust 1.95.0 and each checked-in Cargo.lock, run `cargo test --locked -p openshell-ocsf` in `harry` and `pr4288`. Preserve separate logs. RESULTS.md contains the exact environment used originally.
3. Copy this bundle's `review_trace_downgrade.rs` into `review/crates/openshell-ocsf/tests/`. In `review`, run `cargo test --locked -p openshell-ocsf --features test-support --test review_trace_downgrade -- --nocapture`.
4. In `review`, apply `review-schema-test.patch` with `git apply --unidiff-zero`. It adds one test-only module reusing #4288's existing `sample_events` helper. Production Rust functions stay unchanged. The zero-context patch avoids whitespace-only context lines in the evidence artifact; patch applicability was checked on the exact #4288 tree.
5. Overlay Harry's **exact** OCSF 1.8.0 files into the same relative paths in `review/crates/openshell-ocsf/schemas/ocsf/v1.8.0/`:
   - `classes/{base_event,network_activity,http_activity,ssh_activity,process_activity,detection_finding,application_lifecycle,device_config_state_change,api_activity}.json`
   - `objects/{trace,span,service,key_value_object}.json`
   - `profiles/trace.json`
6. Keep #4288's other schema files, especially `objects/os.json`, `objects/network_connection_info.json`, and all OCSF 1.1.0/1.3.0 definitions. Run `cargo test --locked -p openshell-ocsf --lib review_schema_overlay -- --nocapture`.

The negative log records step 6 before the schema overlay: it correctly rejects Network Activity's undefined `event.trace`. The succeeding matrix after overlay records 54 cases, including 12 kept-native outcomes. This is not a complete implementation rebase and does not test a deployed Collector/Tempo path.

No production fix is supplied in this bundle. The three-check fixture intentionally adds the serialized trace shape to #4288 builder JSON because #4288 alone does not yet include Harry's trace setters. Harry's unchanged suite independently exercises his real setter, serialization, and roundtrip code.
