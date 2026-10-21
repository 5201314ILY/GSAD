# Build verification record

Date: 2026-10-20

Status: **GSAD 0.4.0 has not been compiled in the artifact-authoring environment.** No `rustc`, `cargo`, or `rustfmt` executable was available, and DNS resolution for crates.io was unavailable. The Zstandard dependency therefore could not be fetched here. No successful build or test result is claimed. No `Cargo.lock` was generated because Cargo could not resolve the registry; after a successful build, generate and commit a lockfile for this application workspace.

On a machine with stable Rust and registry access, run from the repository root:

```bash
rustc --version
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo tree -p gsad-core
cargo generate-lockfile
```

Record the exact Rust version, platform, lockfile, resolved dependency versions, and results here after running them. Do not treat ZIP integrity checks or source inspection as a substitute for compile/test results.
