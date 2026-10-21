# GSAD OutputSink 与有界流式输出契约 v0.3

## 目标

当前 `EngineOutput { data: Vec<u8>, ... }` 适合小结果，但不适合任意大小的解压输出、媒体数据和长期网络流。核心因此提供 `OutputWriter` 作为引擎可见的写入接口，并提供 `OutputSink` 作为宿主控制的最终输出端。

原则：**引擎生成数据；核心限制数据；输出端保存数据；核心负责提交或放弃。**

## 类型与职责

- `OutputWriter`：只提供写入与已写字节计数。没有 `commit` / `abort`，防止引擎自行发布未完成的输出。
- `BoundedOutputWriter`：核心实现，在写入前检查取消信号和 `max_output_bytes`，并统计实际接受的字节数。
- `OutputSink`：由宿主提供的目标。实现短写、刷新、提交、放弃与提交语义声明。
- `CommitSemantics`：明确区分原子提交、尽力清理和不可逆输出。
- `execute_to_sink`：统一检查引擎声明、调用引擎、检查输出类型、刷新、提交；任何失败路径都会尝试 `abort`。
- `EngineCompletion`：引擎返回的输出数据类型和元数据，不包含完整数据缓冲。
- `ExecutionReport`：核心返回实际写入字节数、输出类型、元数据与目标提交语义。

## 写入约定

1. `write(&[])` 返回 `Ok(0)`。
2. 非空写入必须返回 `1..=buffer.len()`；返回 `0` 表示无进展并被视为 I/O 错误。
3. 返回值不得大于输入长度。
4. 短写是正常情况。引擎或 `OutputWriter::write_all` 必须继续写入剩余数据。
5. 如果单次请求的完整缓冲区超过剩余输出预算，核心会在该次写入前拒绝它，不会把一个过大的写请求自动截断。大型引擎应使用合理大小的块。
6. 写入计数只统计 `OutputSink.write` 成功接受的字节数，不包括尚未提交的含义或磁盘持久化的证明。

## 生命周期

成功路径：

`validate -> engine.execute_to -> cancellation check -> output-kind check -> sink.flush -> cancellation check -> sink.commit -> report`

失败路径：

`error -> sink.abort (best effort) -> return original error`

如果 `abort` 自身也失败，返回错误会包含原始失败及清理失败的信息。由于不同目标具有不同的事务语义，核心不会声称所有输出均可回滚：本地文件 sink 可通过临时文件加原子发布来避免提前暴露半成品；对象存储可能仅能尽力清理；stdout 或实时网络发送通常不可逆。

## 重要限制

- `execute_to_sink` 不提供操作系统级沙箱，也不保证能强杀卡死的原生引擎。
- 取消是协作式的，引擎必须在合适的安全点检查取消状态，并以小块输出降低取消延迟。
- 默认 `ProcessingEngine::execute_to` 通过旧的 `execute -> Vec<u8>` 路径兼容小输出引擎；这不是大文件的内存安全路径。大输出引擎必须覆写 `execute_to`。
- `max_output_bytes` 在写入边界强制执行；当前不代表全进程内存预算已经强制执行，也不自动实施压缩比限制。
- `commit` 返回错误时，目标是否已经部分提交取决于具体 `OutputSink`。实现必须如实声明 `CommitSemantics`，并对 `abort` 的行为写入自身契约测试。
- 目前接口是同步写入。真正的异步调度、队列容量、跨任务背压与有界缓冲池仍属于后续运行时阶段。

## 当前文件实现：`AtomicFileSink`

1. 在目标目录中通过 `create_new` 创建唯一临时文件。
2. 数据写入临时文件；提交前执行 `flush` 和 `sync_all`。
3. 临时文件关闭后，通过同目录 `hard_link` 创建最终文件名。该操作不会替换已有目标；目标存在时返回 `Conflict`。
4. 发布成功后，尽力删除临时文件名。失败路径关闭并尽力删除临时文件。

此实现提供的是支持硬链接文件系统上的原子可见性，不代表所有文件系统都支持该操作；不支持时提交会失败。当前没有跨平台目录项持久化保证，也没有启动时清理崩溃残留临时文件的机制。相关边界见 `ADR-0004_原子文件输出与提交边界.md`。

## 验收测试

`tests/output_contracts.rs` 覆盖：短写成功、字节上限、引擎中途失败、输出类型声明错误、刷新失败和提交失败。`tests/end_to_end.rs` 增加文件压缩/解压往返、目标不覆盖、格式错误和输出上限失败场景。以上测试仍需在具备 Rust 工具链的环境实际运行。
