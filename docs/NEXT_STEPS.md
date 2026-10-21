# GSAD next engineering steps

## Current baseline — v0.5.0

The `v0.5.0` tag is the baseline for the current development branch.
The `feat/v0.6-task-contract` branch adds task lifecycle contracts,
execution-plan validation, a multi-stage executor, and stronger
`FileSource` snapshot-length boundary behavior.

Local validation on the development branch has passed:

- `cargo fmt --all -- --check`
- `cargo test --workspace` — 54 tests passed
- `cargo clippy --workspace --all-targets -- -D warnings`

These results describe the tested local workspace. Run the same gates
in CI and on supported operating systems before declaring a release.
The `FileSource` length boundary is not a content snapshot: concurrent
file mutation remains unsupported.

## P0 — review and cross-platform verification

- Review the complete branch diff against `v0.5.0`.
- Validate `AtomicFileSink` on Linux and Windows, including existing
  destinations, temporary-file cleanup, hard-link support, and failure paths.
- Document file-sync and directory-entry durability guarantees and limitations.
- Keep the no-overwrite publication behavior.
- Run formatting, workspace tests, and Clippy in CI on supported platforms.

## P1 — remaining execution-contract hardening

- Keep the existing plan-validation, failure-cleanup, cancellation, output-kind,
  and cumulative-output-budget tests as regression gates.
- Clarify structured execution errors and task lifecycle observability.
- Evaluate whether trust requirements and per-stage verification metadata belong
  in a future plan-contract revision.
- Add platform-specific failure-path tests as concrete gaps are identified;
  avoid duplicating scenarios already covered by the workspace suite.
## P2 — benchmark mature codec adapters

Compare Zstandard with DEFLATE and LZ4 on representative corpora. Evaluate
compression ratio, throughput, latency to first output, memory, malformed-input
behavior, dictionary behavior, dependency footprint, licensing, and platform
support. Do not infer a universal winner from one data set.

The configured output and expansion limits are not a hard process-memory
ceiling. Continue reviewing decoder window and resource-limit semantics.

## P3 — storage prototype

Compare a raw object backend, an indexed chunk store, and a content-addressed
chunk store. Use representative benchmarks and crash-injection tests before
deciding whether GSAD needs a native container format.

## P4 — scheduling and backpressure

The current multi-stage executor is synchronous. Design bounded scheduling,
cross-task backpressure, and concurrency semantics only after the synchronous
lifecycle, cancellation, output-publication, and resource-limit contracts
are stable and well tested.

## P5 — specialized adapters

Add a format inspector/parser, then archive support. Evaluate media,
database, cryptographic, protocol-analysis, and REA integrations as independent
adapters under the same core contracts. Each adapter needs tests for malformed
input, bounds, cancellation, resource use, and third-party license obligations.

## Release discipline

- Keep `v0.5.0` as the existing baseline tag.
- Review and validate the complete `feat/v0.6-task-contract` branch before
  merging it.
- Create a new release tag only after the branch is reviewed, integrated,
  and its release-specific validation is complete.
