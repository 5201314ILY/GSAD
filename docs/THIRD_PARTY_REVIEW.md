# Third-party dependency review — pending release audit

This document records the dependency introduced in GSAD 0.4.0. It is not a legal conclusion and does not replace review of the actual resolved dependency tree.

## Zstandard Rust adapter

- Direct dependency: `zstd = "0.14.0"` in `crates/gsad-core/Cargo.toml`.
- Published crate documentation: <https://docs.rs/zstd/0.14.0/zstd/>
- Upstream repository: <https://github.com/gyscos/zstd-rs>
- Upstream Zstandard project: <https://github.com/facebook/zstd>
- The Rust binding and the embedded/upstream C implementation have different licensing notices. Review the exact license metadata and notice files of `zstd`, `zstd-safe`, `zstd-sys`, and the bundled Zstandard source after Cargo resolves the dependency tree.

## Release checklist

- [ ] Generate and commit `Cargo.lock` from a network-connected Rust toolchain.
- [ ] Save the output of `cargo tree -p gsad-core` with the release records.
- [ ] Review license fields and bundled notice files for every resolved dependency.
- [ ] Preserve required third-party notices in the distribution.
- [ ] Select a license for GSAD itself before public release; it is not yet selected in this prototype.
