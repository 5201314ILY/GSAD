# GSAD next engineering steps

## P0 — compile/test gate for 0.4.0

The 0.4.0 source now depends on the `zstd` crate. Before treating it as working software, use a machine with stable Rust and crates.io access:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo tree -p gsad-core
```

The artifact-authoring environment has no `rustc`, `cargo`, or `rustfmt`, and registry DNS is unavailable. Do not claim a successful build until these commands complete.

## P1 — Zstandard integration (added, verification pending)

- `ZstdEngine` wraps the upstream stream API and emits standard Zstandard frames with checksums.
- CLI defaults to Zstandard; `--codec rle` keeps the reference codec available for contract tests.
- Tests cover round trip, frame magic, corrupted checksum, expansion-ratio rejection, and unknown parameters.
- Review memory/window semantics carefully: the configured window limit is not a hard process memory ceiling, and ratio checks use bytes fetched from the source adapter.

## P2 — harden file publication and platform behavior

Validate `AtomicFileSink` on Linux and Windows and document exact guarantees around same-directory hard links, file sync, directory-entry durability, and unsupported filesystems. Keep the no-overwrite behavior.

## P3 — benchmark mature codec adapters

Compare Zstandard with DEFLATE and LZ4 on representative corpora. Evaluate ratio, throughput, latency to first output, memory, malformed-input behavior, dictionary behavior, dependency footprint, licensing, and platform support. Do not infer a universal winner from one data set.

## P4 — plan execution and capability negotiation

Represent a plan as explicit stages with engine ID/version, input/output kinds, parameters, streaming compatibility, memory needs, trust requirements, and verification steps. Validate the complete plan before any engine executes.

## P5 — storage prototype

Compare a raw object backend, an indexed chunk store, and a content-addressed chunk store. Use benchmarks and crash-injection tests before deciding whether GSAD needs a native container format.

## P6 — specialized adapters

Add a format inspector/parser, then archive support; evaluate FFmpeg, 7-Zip, SQLite, cryptographic providers, protocol analyzers, and REA as independent adapters under the same core contract. Each adapter requires tests for malformed input, bounds, cancellation, resource use, and third-party license obligations.
