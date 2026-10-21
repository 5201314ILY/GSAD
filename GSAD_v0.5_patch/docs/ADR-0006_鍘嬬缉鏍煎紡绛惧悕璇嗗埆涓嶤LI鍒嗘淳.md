# ADR-0006：压缩格式签名识别与 CLI 分派

- 状态：提议随 v0.5.0 实施，须由用户本机测试验证
- 日期：2026-10-20

## 背景

v0.4.0 的压缩命令默认使用 Zstandard；解压命令也默认选择 Zstandard。虽然可以手工指定 `--codec rle`，但调用者必须预先知道输入格式，这不利于未来扩展多种编码和解码引擎。

## 决策

1. 压缩操作继续默认使用 Zstandard。
2. 解压操作默认按文件签名选择已支持的解码器。
3. 当前仅识别 Zstandard 标准帧的 `28 B5 2F FD` 魔数和实验性 RLE v1 的 `GSARLE01` 签名。
4. 检测使用独立的顺序读取会话读取前缀，不改变真正解码会话的游标。
5. 未知或过短签名返回明确的 Unsupported 错误，不根据文件扩展名猜测。
6. 用户仍可通过 `--codec zstd` 或 `--codec rle` 显式指定解码器。

## 非目标

此更改不是通用 MIME/文件类型识别器，也不承诺识别 Zstandard skippable frames、任意拼接流、归档、媒体容器、加密容器或网络协议。签名识别只选择解码器；真正的数据完整性仍由对应的解码引擎验证。

## 验收条件

- `cargo fmt --all -- --check` 通过。
- `cargo test --workspace` 通过，包括新增签名识别测试。
- `cargo clippy --workspace --all-targets -- -D warnings` 通过。
- 对 Zstandard 和 RLE 文件，在不指定 `--codec` 的情况下都能选择正确引擎。
- 对未知输入拒绝解码，且不发布输出文件。
