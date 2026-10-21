# GSAD core architecture notes — v0.6

## Core principle

Unify capability description, task and resource contracts, source interfaces,
errors, output lifecycle, execution plans, and observability. Do not unify the
algorithms or internal representations of unrelated data domains.

## Current boundaries

- `DataSource`: a source can create independent readers and may offer random access.
- `FileSource`: captures the file length when opened. Sequential and random reads
  are bounded by that recorded length; premature EOF before the recorded length
  is reported as an error.
- `SourceReader`: sequential read session; short reads are permitted; zero means
  EOF when no more bytes are available within the source's readable boundary.
- `CapabilityDescriptor`: declarative description of an engine's claimed behavior.
- `ProcessingEngine`: in-process engine contract.
- `CapabilityRegistry`: thread-safe registration and lookup, not isolation or scheduling.
- `ExecutionContext`: cooperative cancellation and per-task limits.
- `OutputSink`: host-owned destination lifecycle.
- `AtomicFileSink`: staging and atomic visibility through same-directory hard-link
  publication, with no-overwrite semantics on supported filesystems.
- `RleEngine`: experimental reference codec for exercising the streaming path.
- Zstandard engine: codec adapter using the upstream streaming API to emit standard Zstandard frames.
- `TaskId`, `TaskTransition`, and `TaskLifecycle`: task identity and ordered
  lifecycle transitions; these contracts do not constitute a background scheduler.
- `ExecutionPlan` and `ValidatedExecutionPlan`: multi-stage plan description and
  pre-execution validation. A validated plan retains its resolved engine instances.
- Plan executor: runs stages sequentially, uses temporary files for intermediate
  outputs, propagates cancellation and output limits, and commits the final sink
  only after the relevant output checks and flush succeed.
- `gsad` CLI: a small front end for file-to-file compression and decompression;
  Zstandard is the default codec and `--codec rle` selects the reference codec.

## Limits and caveats

1. Per-task limits are enforced at defined source/output boundaries and by each
   engine. There is no OS sandbox or process-wide memory budget.
2. Registry descriptors are claims; registration does not prove an engine correct
   or safe.
3. Legacy `EngineOutput` still materializes bytes in memory. Large-output engines
   should implement `execute_to`.
4. `FileSource` records a length boundary, not an immutable content snapshot.
   Appended bytes beyond that boundary are not read; premature EOF caused by
   truncation is reported. Concurrent content modification, including same-length
   rewrites, is unsupported.
5. The engine trait, sink interface, and multi-stage executor are synchronous.
   No async scheduler, bounded work queue, or cross-task backpressure runtime exists.
6. `AtomicFileSink` depends on same-directory hard-link support. It syncs file
   contents but does not guarantee directory-entry durability after power loss
   on all platforms.
7. `GSARLE01` is a no-checksum reference format, not an archive-grade or
   general-purpose codec.
8. No GSAD-owned persistent object/container format is defined.
9. Intermediate workspace cleanup is best-effort. Validate cleanup behavior
   under platform-specific filesystem failures.
10. Cooperative cancellation depends on engines checking the cancellation
    context. It is not forced preemption of a running engine.

## Extension sequence

1. Keep formatting, workspace tests, and Clippy as required validation
   gates for all changes.
2. Validate atomic file output, file mutation behavior, and cleanup on Linux
   and Windows.
3. Improve structured execution observability and continue hardening plan,
   cancellation, and resource-limit contracts.
4. Benchmark mature codec adapters on representative datasets.
5. Design bounded scheduling and cross-task backpressure after the synchronous
   contracts are stable.
6. Prototype raw-object, indexed-chunk, and content-addressed storage backends.
   Use benchmarks and crash-injection tests before choosing a native format.
7. Add format, archive, media, crypto, network-protocol, database, and REA
   adapters behind capability contracts.

See `OUTPUT_SINK.md`, `GSARLE01_FORMAT.md`,
`ADR-0003_项目正式更名与命名规则.md`, and
`ADR-0004_原子文件输出与提交边界.md`.
