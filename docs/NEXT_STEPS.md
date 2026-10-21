# GSAD next engineering steps

## Current state — v0.6.0 released

GSAD v0.6.0 has been published on GitHub. The workspace packages target
`0.6.0`; keep the `v0.6.0` tag and release as immutable historical
artifacts.

The release includes task lifecycle contracts, execution-plan validation,
the multi-stage executor, and stronger `FileSource` snapshot-length
boundary behavior. This length boundary is not a content snapshot:
concurrent file mutation remains unsupported.

The current hardening work is on `feat/atomic-sink-hardening`, based on
`v0.6.0`. It strengthens `AtomicFileSink` commit-failure cleanup and adds
dedicated lifecycle and publication tests.

Local validation on the current Windows development environment has passed:

- `cargo fmt --all -- --check`
- `cargo test --workspace` — 60 tests passed
- `cargo clippy --workspace --all-targets -- -D warnings`

Run the full validation suite before integrating this branch. Verify the
branch in Linux and Windows CI after pushing it. These hardening changes
have not yet been integrated into `master`.
## P0 — review and cross-platform verification

- Linux and Windows CI passed for formatting, workspace tests, and Clippy.
- Add targeted tests for filesystem-specific failures that are not yet
  directly simulated, including unsupported hard-link publication and
  persistent temporary-file cleanup failures.
- Review file-sync and directory-entry durability documentation against
  the implementation. Directory-entry durability after sudden power loss
  remains an explicit limitation.

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

- Retain `v0.5.0` as the historical baseline tag.
- The `v0.6.0` tag and GitHub Release are already published; do not retarget or recreate them.
- Create a future release tag only after the relevant branch is reviewed, integrated, and its release-specific validation and notes are complete.
