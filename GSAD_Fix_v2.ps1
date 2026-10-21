$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath '.\Cargo.toml')) {
    throw 'Run this script from the GSAD workspace root, the directory containing Cargo.toml.'
}

$targets = @(
    'crates\gsad-core\src\zstd_codec.rs',
    'crates\gsad-core\examples\read_memory.rs',
    'crates\gsad-core\tests\contracts.rs',
    'crates\gsad-core\tests\end_to_end.rs'
)
foreach ($target in $targets) {
    if (-not (Test-Path -LiteralPath $target)) {
        throw "Required file not found: $target. No files were changed."
    }
}

function Replace-ExactOnce {
    param(
        [string]$Text,
        [string]$Old,
        [string]$New,
        [string]$Label
    )
    $count = [regex]::Matches($Text, [regex]::Escape($Old)).Count
    if ($count -ne 1) {
        throw "Patch stopped at '$Label': expected one match, found $count. No source files have been changed."
    }
    return $Text.Replace($Old, $New)
}

# Read and stage every change in memory before writing any source file.
$staged = @{}
foreach ($target in $targets) {
    $text = Get-Content -LiteralPath $target -Raw -Encoding UTF8
    $text = $text -replace "`r`n?", "`n"
    $staged[$target] = $text
}

$path = 'crates\gsad-core\src\zstd_codec.rs'
$oldEncoder = @(
    '        let mut encoder = zstd::stream::write::Encoder::new(&mut output_writer, level)',
    '            .map_err(|error| map_io_error(error, ErrorKind::Io, "zstd.encoder.create"))?;'
) -join "`n"
$newEncoder = @(
    '        let mut encoder = zstd::stream::write::Encoder::new(&mut output_writer, level)',
    '            .map_err(|error| map_io_error(error, ErrorKind::Io, "zstd.encoder.create"))?;',
    '        // Keep the frame window within this task memory budget so the same',
    '        // GSAD engine can decode frames produced under this policy.',
    '        encoder',
    '            .window_log(maximum_window_log(context.limits.max_memory_bytes))',
    '            .map_err(|error| map_io_error(error, ErrorKind::ResourceLimit, "zstd.encoder.window"))?;'
) -join "`n"
$staged[$path] = Replace-ExactOnce $staged[$path] $oldEncoder $newEncoder 'Zstandard encoder window configuration'

$path = 'crates\gsad-core\examples\read_memory.rs'
$staged[$path] = Replace-ExactOnce $staged[$path] 'use gsad_core::{DataSource, SourceReader, source::MemorySource};' 'use gsad_core::{DataSource, source::MemorySource};' 'unused SourceReader in example'

$path = 'crates\gsad-core\tests\contracts.rs'
$staged[$path] = Replace-ExactOnce $staged[$path] 'ResourceLimits, SourceReader, TaskState, GsadError,' 'ResourceLimits, TaskState, GsadError,' 'unused SourceReader in contracts test'

$path = 'crates\gsad-core\tests\end_to_end.rs'
$staged[$path] = Replace-ExactOnce $staged[$path] 'ExecutionContext, Operation, ResourceLimits, RleEngine, SourceReader,' 'ExecutionContext, Operation, ResourceLimits, RleEngine,' 'unused SourceReader in end-to-end test'

# All edits matched exactly. Create a backup before writing staged files.
$backupDir = Join-Path (Get-Location).Path ('.gsad-backup-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $backupDir | Out-Null
foreach ($target in $targets) {
    $backupPath = Join-Path $backupDir $target
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $backupPath) | Out-Null
    Copy-Item -LiteralPath $target -Destination $backupPath
}

$utf8NoBom = [System.Text.UTF8Encoding]::new($false)
foreach ($target in $targets) {
    [System.IO.File]::WriteAllText((Resolve-Path -LiteralPath $target).Path, $staged[$target], $utf8NoBom)
}

Write-Host 'Patch applied.' -ForegroundColor Green
Write-Host "Backup directory: $backupDir"
Write-Host 'Next run these commands:' -ForegroundColor Cyan
Write-Host '  cargo fmt --all'
Write-Host '  cargo fmt --all -- --check'
Write-Host '  cargo test --workspace'
Write-Host '  cargo clippy --workspace --all-targets -- -D warnings'
