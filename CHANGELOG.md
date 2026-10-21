# Changelog

## 0.4.0 — Zstandard adapter

- Added `ZstdEngine` through the published `zstd` crate, using the standard Zstandard frame format.
- Compression writes a frame content checksum; decompression sets a maximum back-reference window from the task memory budget.
- Enforced the task input limit in the source-to-codec adapter and expansion-ratio checks in the decode output path; the absolute output cap remains enforced by the host writer.
- Updated `gsad` CLI to default to Zstandard and to support `--codec zstd|rle` and `--level N` for Zstandard compression.
- Added Zstandard streaming round-trip, checksum corruption, expansion-ratio, and parameter validation tests.
- Actual compile/test remains unverified in this artifact-authoring environment because Rust tooling and registry access are unavailable.

## 0.3.0 — GSAD rename + first complete file workflow

- Renamed the project and Rust package namespace from the temporary UDE name to **广义流矢解码 — Generalized Stray-Arrow Decoding (GSAD)**.
- Added `FileSource` for local-file sequential and random access.
- Added `AtomicFileSink`, which stages output in a same-directory temporary file and publishes without replacing an existing destination on filesystems that support hard links.
- Added experimental streaming `RleEngine` with the `GSARLE01` test format.
- Added `gsad` CLI commands for compress/decompress and end-to-end tests for round trip, no-overwrite behavior, malformed inputs, and output limits.
- Added project rename and codec-format decision records.
- Compilation and tests remain unverified in the artifact-authoring environment because the Rust toolchain is unavailable.

## 0.2.0 — pre-rename prototype

- Introduced bounded streaming output and host-owned sink commit/abort lifecycle under the temporary UDE project name.
