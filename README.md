# 广义流矢解码 — Generalized Stray-Arrow Decoding (GSAD)

> 工程原型 v0.4.0。公共 API 仍处于实验阶段，不承诺稳定性。

GSAD 是一套通用数据处理与存储软件的起点。它的目标是让文本、二进制、压缩数据、归档、媒体、网络协议、密码学和分析工具通过统一的能力契约协同工作，而不是强迫所有数据使用一种算法或一种物理存储格式。

## 本版本包含

- `gsad-core`：数据源抽象、能力描述与注册、任务状态、错误和资源限制。
- `FileSource`：本地文件的顺序读取和随机读取入口。
- `OutputSink` / `BoundedOutputWriter`：有界输出、取消检查，以及由核心控制的刷新、提交和失败清理。
- `AtomicFileSink`：先写同目录临时文件，再尝试以同文件系统硬链接原子发布；默认绝不覆盖已存在的目标文件。
- `ZstdEngine`：基于成熟 `zstd` Rust crate 的流式 Zstandard 压缩与解压适配器。压缩端写入帧内容校验和，解压端按任务限制输入、输出与窗口大小，并可限制解压扩张比。
- `RleEngine`：无第三方依赖的流式 RLE 参考引擎，用于契约验证，不作为通用压缩格式。
- `gsad` CLI：默认使用 Zstandard，可用 `--codec rle` 显式选择实验性 RLE。
- 契约测试、端到端测试、CI 配置和架构决策记录。

## 快速使用

需要当前稳定版 Rust 工具链。在项目根目录执行：

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p gsad-cli -- help
```

默认使用 Zstandard：

```bash
cargo run -p gsad-cli -- compress input.bin output.zst
cargo run -p gsad-cli -- decompress output.zst restored.bin
```

指定压缩级别（支持范围由所用 Zstandard 库报告）：

```bash
cargo run -p gsad-cli -- compress --codec zstd --level 6 input.bin output.zst
```

旧版实验性 RLE 只用于契约测试，必须显式指定同一编解码器：

```bash
cargo run -p gsad-cli -- compress --codec rle input.bin output.gsarr
cargo run -p gsad-cli -- decompress --codec rle output.gsarr restored.bin
```

输出目标若已存在，工具会拒绝覆盖。命令行原型默认最大输入 256 MiB、最大输出 512 MiB、声明的任务内存预算 256 MiB。Zstandard CLI 配置采用最大扩张比 1,000,000，以允许高压缩率数据往返；显式选择 RLE 时使用核心默认值 200。CLI 当前尚未提供调整这些限制的参数。

## 格式与安全边界

Zstandard 输出是标准 Zstandard frame 流，不是 GSAD 自有容器。压缩端要求写入内容校验和；解压端如果遇到带校验和的帧，会由解码器验证。解码端允许标准解码器支持的多帧流。具体行为、窗口限制与错误处理见 `docs/ZSTD_ENGINE.md`。

RLE 的 `GSARLE01` 格式只是参考格式，无内置校验和，不应用于长期归档。具体格式见 `docs/GSARLE01_FORMAT.md`。

`AtomicFileSink` 在支持同目录硬链接的文件系统上提供原子可见性，并且不会替换已有目标；如果文件系统不支持硬链接，提交会失败。文件内容在发布前会 `sync_all`，但目前没有跨平台目录项持久化保证，因此不能承诺断电后绝对持久。失败时会尽力删除临时文件。

当前任务内存限制并不是通用进程级内存沙箱。Zstandard 的窗口限制约束回溯窗口，但无法精确限制依赖库的所有分配；绝对输出限制仍是强制约束，扩张比依据数据源已读取字节估算，在解码器读前缓冲下可能略宽松。面对恶意输入时，还需要进程隔离、操作系统资源限制与模糊测试。

核心尚不包含完整的通用任务调度器、持久任务队列、统一存储格式、进程级内存预算、恶意原生插件沙箱，也尚未接入 FFmpeg、7-Zip、SQLite 或 REA。项目许可证仍未选定；第三方依赖的许可证与构建链需要在发布前审核。当前尚未生成 `Cargo.lock`；首次在可联网环境解析依赖后应将其提交。发布审查清单见 `docs/THIRD_PARTY_REVIEW.md`。

## 设计原则

**统一管理与契约，不统一所有算法。** 字符解码、结构解析、解压缩、媒体解码、协议分帧和密码学运算是不同的能力。它们可以共享数据源、调度、取消、资源控制、验证和可观测性，但必须保留自己的语义与安全约束。

存储也采用同样原则：统一逻辑对象与访问契约，允许不同数据使用原始存储、分块、索引、内容寻址、增量更新或专业格式。最终是否创建 GSAD 自有容器格式，需要由明确的需求、基准测试和故障恢复测试决定。

## 工程布局

```text
.
├── Cargo.toml
├── crates/
│   ├── gsad-core/
│   │   ├── src/             # 核心契约、数据源、输出、RLE 与 Zstandard 引擎
│   │   ├── tests/           # 契约与端到端测试
│   │   └── examples/
│   └── gsad-cli/            # gsad 命令行原型
└── docs/                    # 架构、API、ADR、格式与验证记录
```

## 验证状态

当前创作环境没有可用的 `rustc`、`cargo` 或 `rustfmt`，也无法从 crates.io 下载依赖，因此本版本仍**未完成实际编译与测试验证**。这里也暂时无法生成 `Cargo.lock`；首次在可联网的 Rust 环境中成功解析依赖后，应将锁文件纳入仓库。不能把源码审查或 ZIP 完整性检查当成编译通过。请在联网且装有 Rust stable 的机器上执行上述命令，并记录实际版本与结果。CI 已配置 GitHub Actions 的格式、测试和 Clippy 检查。
