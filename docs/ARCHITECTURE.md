# GSAD core architecture notes — prototype v0.3

## Core principle

Unify capability description, task/resource contracts, source interfaces, errors, output lifecycle, and observability. Do not unify the algorithms or internal representations of unrelated data domains.

## Current boundaries

- `DataSource`: a source can create independent readers and may offer random access.
- `FileSource`: local file source with a snapshot length; callers should not modify the file during processing.
- `SourceReader`: sequential read session; short reads are permitted; zero means EOF for a non-empty requested buffer.
- `CapabilityDescriptor`: declarative description of an engine's claimed behavior.
- `ProcessingEngine`: in-process engine contract.
- `CapabilityRegistry`: thread-safe registration and lookup, not isolation or scheduling.
- `ExecutionContext`: cooperative cancellation and per-task limits.
- `OutputSink`: host-owned destination lifecycle.
- `AtomicFileSink`: staging and atomic visibility through same-directory hard-link publication, with no-overwrite semantics on supported filesystems.
- `RleEngine`: experimental reference codec for exercising the full streaming path.
- `TaskState`: allowed lifecycle transitions; not yet a task executor.
- `gsad` CLI: a small front end demonstrating file-to-file compression/decompression with the RLE reference format.

## Limits and caveats

1. Per-task limits are only enforced at the source/output boundaries and by each engine. There is no OS sandbox or process-wide memory budget.
2. Registry descriptors are claims; registration does not prove an engine correct or safe.
3. Legacy `EngineOutput` still materializes bytes in memory. Large-output engines must implement `execute_to`.
4. File source length is captured at construction time; concurrent file mutation is unsupported.
5. The engine trait and sink interface are synchronous. No async scheduler, bounded work queue, or cross-task backpressure runtime exists yet.
6. `AtomicFileSink` depends on same-directory hard-link support. It syncs file contents but does not guarantee directory-entry durability after power loss on all platforms.
7. `GSARLE01` is a no-checksum reference format, not an archive-grade or general-purpose codec.
8. No GSAD-owned persistent object/container format is defined.

## Extension sequence

1. Run formatting, compile, unit/integration test, and Clippy gates on Rust stable.
2. Validate atomic file output on Linux and Windows, including existing destinations and temp-file cleanup.
3. Add a mature codec adapter, comparing ratio, speed, memory, portability, licensing and malformed-input handling.
4. Define plan validation, task identity and structured events.
5. Add bounded scheduling and cross-task backpressure after synchronous lifecycle contracts are tested.
6. Add format, archive, media, crypto, network-protocol, database and REA adapters behind capability contracts.
7. Benchmark representative datasets before choosing a native GSAD storage format.

See `OUTPUT_SINK.md`, `GSARLE01_FORMAT.md`, `ADR-0003_项目正式更名与命名规则.md`, and `ADR-0004_原子文件输出与提交边界.md`.
