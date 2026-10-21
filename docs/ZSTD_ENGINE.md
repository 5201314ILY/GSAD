# GSAD Zstandard engine

Status: prototype adapter in GSAD 0.4.0; not a GSAD-owned codec or container format.

## Dependency

The core crate uses `zstd` Rust crate version range `0.14.x` (`zstd = "0.14.0"`). The crate wraps the upstream Zstandard implementation and provides streaming Read/Write APIs. Before release, review the exact resolved dependency tree and applicable license notices.

## Wire format

The engine emits standard Zstandard frame data. It does not add a GSAD-specific header, object ID, or storage index. Compression enables the Zstandard frame content checksum. The decoder validates it when one exists in the input frame; old frames without checksums remain decodable. The stream decoder accepts the frame behavior supported by the upstream stream API, including concatenated frames.

## Engine contract

- Capability ID: `gsad.codec.zstd`
- Operations: `Compress`, `Decompress`
- Input/output kind: `Bytes`
- Streaming: yes
- Random access: no
- Compression parameter: `level` as a signed integer, checked against the range exposed by the linked Zstandard library. Default is level 3.
- Unknown compression parameters and all decompression parameters are rejected in this prototype.

## Resource controls

1. The `SourceIoReader` checks cancellation before reads and enforces `max_input_bytes`, probing at most one additional byte to distinguish an exact-limit EOF from overflow.
2. The core `BoundedOutputWriter` enforces `max_output_bytes` on all bytes written to the sink.
3. During decompression, a ratio guard compares output bytes with bytes fetched from the source adapter. Since the underlying decoder may read ahead into its internal buffer, this estimate can be somewhat lenient. The generic core default remains 200. The CLI deliberately selects 1,000,000 for Zstandard so highly compressible valid streams can round-trip under the CLI preset; API callers should choose a per-task ratio appropriate for their trust boundary. It is not a replacement for the strict absolute output bound.
4. The decoder's maximum back-reference window is derived from `max_memory_bytes`. This limits the Zstandard window, but does not constitute a hard cap on the codec's complete memory use, allocations outside the window, or process RSS. Process isolation and OS resource controls remain future work.
5. Cancellation is cooperative and checked between transfer reads. A currently executing native codec operation may not be interrupted immediately.

## Failure and output publication

All output is written through the host's bounded output writer into an `OutputSink`. The host commits only after the engine completes, reported output kind is checked, the sink flushes, and cancellation is checked. On decode error or limit violation, the host attempts to abort the sink. With `AtomicFileSink`, this prevents publication of incomplete output when its cleanup and filesystem assumptions hold.

## Tests added

- Round trip of a large mixed/repetitive byte stream, a highly compressible zero stream, and empty input.
- Standard Zstandard frame magic verification.
- Corruption of the frame's final checksum bytes is rejected when the decoder verifies the checksum.
- Expansion-ratio violations abort without publishing the destination.
- Unknown parameters are rejected before a target is published.

These tests are written but not yet executed in the artifact-authoring environment because `cargo`/`rustc` are unavailable and the crate registry cannot be resolved from that environment.
