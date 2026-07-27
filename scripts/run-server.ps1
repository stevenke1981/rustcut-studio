$ErrorActionPreference = "Stop"
$Root = Resolve-Path (Join-Path $PSScriptRoot "..")
if (-not $env:RUSTCUT_DATA_DIR) { $env:RUSTCUT_DATA_DIR = Join-Path $Root "data" }
& (Join-Path $Root "bin\rustcut-server.exe") @args
exit $LASTEXITCODE
