# GSAD v0.5.0 patch — automatic format detection

This is an incremental patch for an existing GSAD v0.4.0 workspace. It intentionally updates only:

- `Cargo.toml`: workspace version `0.4.0` -> `0.5.0`
- `crates/gsad-cli/src/main.rs`: CLI default behavior and format detection tests

It does not replace the core crate, so it preserves the Zstandard window-limit fix already applied in your local workspace.

## Apply

1. Copy `GSAD_Patch_v0.5.0.ps1` into the GSAD workspace root (the directory containing `Cargo.toml`).
2. In PowerShell, run:

   ```powershell
   cd D:\Future\GSAD-Total-Item\GSAD
   powershell -NoProfile -ExecutionPolicy Bypass -File .\GSAD_Patch_v0.5.0.ps1
   ```

3. Run the verification commands shown at the end of the script.

A backup of the old manifest and CLI source is created before either file is changed.

## Behavior change

- `compress` defaults to Zstandard as before.
- `decompress` defaults to `--codec auto`.
- Auto detection uses file magic, not extension:
  - Zstandard standard frame: `28 B5 2F FD`
  - GSAD experimental RLE v1: `GSARLE01`
- Unknown or too-short signatures are rejected instead of guessed.
- Explicit `--codec zstd` or `--codec rle` remains available.

This is signature dispatch for two formats only, not a general-purpose file-format detector. It does not imply support for all Zstandard skippable frames, concatenated-stream policies, archives, media, encryption or network protocols.
