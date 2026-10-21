$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath '.\Cargo.toml')) {
    throw '请先 cd 到 GSAD 工作区根目录（包含 Cargo.toml 的目录）再运行此脚本。'
}

$files = @(
    'crates\gsad-core\src\zstd_codec.rs',
    'crates\gsad-core\examples\read_memory.rs',
    'crates\gsad-core\tests\contracts.rs',
    'crates\gsad-core\tests\end_to_end.rs'
)
foreach ($file in $files) {
    if (-not (Test-Path -LiteralPath $file)) {
        throw "找不到预期文件：$file。请确认你在正确的 GSAD 项目根目录。"
    }
}

$backupDir = Join-Path (Get-Location).Path ('.gsad-backup-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $backupDir | Out-Null
foreach ($file in $files) {
    $target = Join-Path $backupDir $file
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
    Copy-Item -LiteralPath $file -Destination $target
}

function Replace-ExactOnce {
    param(
        [Parameter(Mandatory = $true)][string]$Text,
        [Parameter(Mandatory = $true)][string]$Old,
        [Parameter(Mandatory = $true)][string]$New,
        [Parameter(Mandatory = $true)][string]$Label
    )
    $count = [regex]::Matches($Text, [regex]::Escape($Old)).Count
    if ($count -ne 1) {
        throw "补丁无法安全应用：'$Label' 预期出现一次，实际出现 $count 次。未继续修改该文件。"
    }
    return $Text.Replace($Old, $New)
}

$script:utf8NoBom = New-Object -TypeName System.Text.UTF8Encoding -ArgumentList $false
function Save-Utf8NoBom {
    param([string]$Path, [string]$Text)
    [System.IO.File]::WriteAllText((Resolve-Path -LiteralPath $Path).Path, $Text, $script:utf8NoBom)
}

$path = 'crates\gsad-core\src\zstd_codec.rs'
$text = Get-Content -LiteralPath $path -Raw -Encoding UTF8
$text = Replace-ExactOnce $text 'const MAX_WINDOW_LOG: u32 = 31;' 'const MAX_WINDOW_LOG: u32 = 23; // Conservative interoperability ceiling: 8 MiB.' 'Zstandard window ceiling'
$oldEncoder = @(
    '        let mut encoder = zstd::stream::write::Encoder::new(&mut output_writer, level)',
    '            .map_err(|error| map_io_error(error, ErrorKind::Io, "zstd.encoder.create"))?;'
) -join "`n"
$newEncoder = @(
    '        let mut encoder = zstd::stream::write::Encoder::new(&mut output_writer, level)',
    '            .map_err(|error| map_io_error(error, ErrorKind::Io, "zstd.encoder.create"))?;',
    '        // Match the encoder window to the decoder's task budget. Otherwise a',
    '        // streamed frame may advertise a default 8 MiB window and be rejected',
    '        // by this same engine when the task budget is smaller.',
    '        encoder',
    '            .window_log(maximum_window_log(context.limits.max_memory_bytes))',
    '            .map_err(|error| map_io_error(error, ErrorKind::ResourceLimit, "zstd.encoder.window"))?;'
) -join "`n"
$text = $text -replace "`r`n", "`n"
$text = Replace-ExactOnce $text $oldEncoder $newEncoder 'Zstandard encoder window setting'
Save-Utf8NoBom $path $text

$path = 'crates\gsad-core\examples\read_memory.rs'
$text = Get-Content -LiteralPath $path -Raw -Encoding UTF8
$text = Replace-ExactOnce $text 'use gsad_core::{DataSource, SourceReader, source::MemorySource};' 'use gsad_core::{DataSource, source::MemorySource};' 'unused SourceReader import in read_memory example'
Save-Utf8NoBom $path $text

$path = 'crates\gsad-core\tests\contracts.rs'
$text = Get-Content -LiteralPath $path -Raw -Encoding UTF8
$text = Replace-ExactOnce $text 'ResourceLimits, SourceReader, TaskState, GsadError,' 'ResourceLimits, TaskState, GsadError,' 'unused SourceReader import in contracts test'
Save-Utf8NoBom $path $text

$path = 'crates\gsad-core\tests\end_to_end.rs'
$text = Get-Content -LiteralPath $path -Raw -Encoding UTF8
$text = Replace-ExactOnce $text 'ResourceLimits, RleEngine, SourceReader,' 'ResourceLimits, RleEngine,' 'unused SourceReader import in end_to_end test'
Save-Utf8NoBom $path $text

Write-Host ''
Write-Host '补丁已应用；修改前的 4 个文件已备份到：' -ForegroundColor Green
Write-Host $backupDir
Write-Host ''
Write-Host '现在请依次运行：' -ForegroundColor Cyan
Write-Host '  cargo fmt --all'
Write-Host '  cargo fmt --all -- --check'
Write-Host '  cargo test --workspace'
Write-Host '  cargo clippy --workspace --all-targets -- -D warnings'
