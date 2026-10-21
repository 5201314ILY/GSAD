# ADR-0005: Zstandard 引擎接入与 CLI 默认格式

- 状态：Proposed / implementation added, pending compile and test verification
- 日期：2026-10-20

## 背景

GSAD 0.3.0 只有实验性 RLE codec，无法证明系统支持通用压缩场景，也不适合作为默认文件格式。我们需要把一个成熟的压缩算法接入统一引擎契约，验证流式输入、流式输出、资源限制、校验和与失败提交是否协同。

## 决策

1. 增加 `ZstdEngine`，通过 Rust `zstd` crate 的流式 API 实现，不在 GSAD 内部重写 Zstandard 算法。
2. 输出采用标准 Zstandard frame，不增加自有包装层；压缩默认启用 frame checksum。
3. CLI 的默认 codec 调整为 `zstd`，并通过 `--codec rle` 保留实验性格式的显式访问。
4. 支持 `--level N` 仅用于 Zstandard 压缩；级别有效范围以依赖库提供的范围为准。
5. 解码执行输入上限、绝对输出上限、窗口上限与可选扩张比检查；核心通用默认扩张比保留为 200；CLI 选择 Zstandard 时将该比例放宽到 1,000,000，以便默认压缩/解压流程能够往返高压缩率数据。512 MiB 默认绝对输出上限仍然严格生效；处理不可信数据的 API 调用方应显式设置更严格的每任务比例。
6. 保持压缩算法由专业库负责，GSAD 核心管理任务契约、资源约束和输出提交。

## 原因

Zstandard 提供成熟的流式 Read/Write 接口和可验证标准帧；通过适配器接入，比自写通用压缩算法更合理。标准帧也避免过早为 GSAD 定义专有的压缩包装格式。

## 代价与风险

- 新增原生构建依赖与 Cargo registry 依赖，完整构建需要合适的 Rust 工具链和 C 构建工具链。
- frame checksum 可发现意外损坏，但不是密码学认证，不能抵御恶意篡改者。
- 解压扩张比使用读取计数估算，底层读前缓冲可能导致轻微宽松；绝对输出上限仍然严格限制实际写出量。
- 内存窗口限制不是硬性进程 RSS 上限。
- 本 ADR 在实际编译、测试和许可证审查通过前不应视为最终批准。

## 验证要求

- `cargo fmt --all -- --check`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- 在 Linux 和 Windows 至少一个目标上验证压缩/解压、校验和失败、超限清理与输出目标不覆盖。
- 检查实际解析后的依赖树与许可证公告。
