# Pre-aborted OAuth token acquisition: downstream evidence

## Scope

An uncached `clientCredentials().getToken(AbortSignal.abort())` already rejects with `SdkError('canceled')`. Before this change, it first started discovery. A discovery failure then rejected the shared exchange without a waiter, terminating a Node consumer despite that consumer handling cancellation.

The candidate adds one guard in `sdk/typescript/src/oidc.ts`, immediately after the unchanged cached-token fast path. It prevents starting an exchange for an already-aborted acquisition waiter. Cached-token behavior, active waiter cancellation, and live caller coalescing remain unchanged. Three adjacent tests cover fresh pre-abort, cached behavior, and mixed canceled/live failure with later retry.

This is a downstream candidate, not an upstream submission or maintainer acceptance. The implementation being corrected was introduced by Seth Jennings in [OpenShell PR #2907](https://github.com/NVIDIA/OpenShell/pull/2907), commit `5206bc51b2257cf36ac1cdd9fd8e4567db204b3a`, closing [issue #2803](https://github.com/NVIDIA/OpenShell/issues/2803).

## Native consumer proof

`oidc-preaborted-consumer.mjs` imports the built package's public root export, `dist/index.js`, and uses a real localhost HTTP endpoint returning a synthetic 503. It does not replace or mock the SDK, native fetch, error modules, or HTTP transport. All fixture identifiers and credentials are synthetic.

The consumer handles the expected cancellation, allows asynchronous discovery failure to settle, closes local server connections, and awaits server closure. Its request timeout is 100 ms; its post-cancellation observation is 250 ms. `run-consumer-with-deadline.mjs` launches it with strict unhandled-rejection behavior, records the child's actual close event, and applies a 5-second process deadline if it hangs.

The exact same consumer, unit-test bundle, dependency lock, and runtime were run against fresh separate baseline and candidate builds:

- Baseline `eeba0e7954c0fb4d8e9e2e29d1bfa68e290eb8b3`: caught cancellation, then an unhandled OAuth discovery HTTP 503; child exit 1 in 296 ms. The deadline was not reached.
- Candidate: caught cancellation, made zero discovery requests, closed its server, and exited 0 in 497 ms. The deadline was not reached.

A normal child close before the deadline distinguishes completed teardown from an outstanding request or idle server handle. The native proof is distinct from the adjacent unit controls. Their native-fetch spy observes calls without replacing fetch, and their fixtures exercise successful shared acquisition, failure propagation, and later retry.

## Reproduce

From an OpenShell checkout with Node 24 and npm available:

```shell
export XDG_CACHE_HOME="$PWD/.cache"
(cd sdk/typescript && npm ci --cache "$XDG_CACHE_HOME/npm" && npm run gen && npm run build)
node reviews/oidc-preabort-20261010/run-consumer-with-deadline.mjs \
  reviews/oidc-preabort-20261010/oidc-preaborted-consumer.mjs \
  "$PWD/sdk/typescript/dist/index.js"
(cd sdk/typescript && npm exec -- vitest run src/oidc.test.ts)
(cd sdk/typescript && node ../../reviews/oidc-preabort-20261010/run-independent-with-deadline.mjs \
  15000 node --unhandled-rejections=strict --input-type=module \
  --eval "$(cat ../../reviews/oidc-preabort-20261010/native-lifecycle-controls.mjs)")
```

For the baseline differential, use a separate worktree at the pinned base, copy the same consumer/harness and final `oidc.test.ts` into it, install from its unchanged lockfile, regenerate wire types, and build its own `dist/`. Do not reuse a candidate build as a baseline.

The independent lifecycle oracle uses the package's `@nvidia/openshell-sdk` self-reference. Run its exact script content from `sdk/typescript`, as shown above; launching that file directly from the top-level `reviews/` directory would not resolve the package self-reference.

## Verification and provenance

- Final focused OIDC suite: 24 passed; baseline: 1 failed and 23 passed controls.
- Full TypeScript SDK suite: 183 passed, 92.92% line coverage.
- Type check, Biome, build, and protobuf lint: passed.
- Runtime: Node v24.19.0, npm 11.9.0.
- No Rust, Python, gateway, container, or deployment E2E result is claimed; those paths are unchanged.

An initial wider experiment required cached-token calls to honor pre-aborted signals. That behavior lacked an explicit established public contract and was not adopted. The final guard preserves the original cached fast path. The final baseline/candidate bundle was frozen and rerun identically.

An independent isolated source/build replay reproduced the strict consumer's baseline exit 1 in 301 ms and candidate exit 0 in 525 ms, both without a deadline hit. It reran all 24 OIDC tests, all 183 SDK tests, type check, Biome, build, and protobuf lint. Its additional native package-self-reference oracle passed six lifecycle controls on both baseline and candidate: concurrent/cache behavior, mixed canceled/live waiters, all started waiters canceled before a token 503, canceled acquisition completing successfully, stalled discovery timeout, and an asynchronous secret-supplier rejection. Both oracle processes closed normally in about 1.1 seconds and every localhost server was closed.

Setup failures are distinguished from product failures. Original system time instrumentation was unavailable and was replaced by the included Node deadline harness. The independent protobuf-lint attempt initially failed because its default home cache was read-only; rerunning with a sandbox-local `XDG_CACHE_HOME` passed. That failed attempt is retained alongside the successful retry. No failed setup is counted as a passed gate.

`logs.json` separates original and independent runs. It retains exact diagnostic content except absolute sandbox checkout prefixes, which are explicitly replaced by `<BASELINE_CHECKOUT>`, `<CANDIDATE_CHECKOUT>`, and `<INDEPENDENT_CHECKOUT>`, and the one default Buf cache path, replaced by `<DEFAULT_CACHE>/buf`. The same cache-path replacement is disclosed in the setup-failure review summary. Raw local originals are preserved separately. `manifest.json` identifies the exact baseline, final source/tests, runtime and evidence bytes. Independent source/runtime review certification is recorded separately in `review.json`.
