$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

$versionMatch = Select-String -Path "Cargo.toml" -Pattern '^version = "([^"]+)"' | Select-Object -First 1
$version = if ($versionMatch) { $versionMatch.Matches[0].Groups[1].Value } else { "0.1.0" }
$name = "rustcut-studio-$version-windows-$env:PROCESSOR_ARCHITECTURE"
$stage = Join-Path "dist" $name

cargo test --workspace
cargo build --release --workspace
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item (Join-Path $stage "bin") -ItemType Directory -Force | Out-Null

Copy-Item "target/release/rustcut-cli.exe" (Join-Path $stage "bin")
Copy-Item "target/release/rustcut-server.exe" (Join-Path $stage "bin")
Copy-Item "target/release/rustcut-mcp.exe" (Join-Path $stage "bin")
Copy-Item "web", "config", "docs", "deploy", "scripts" $stage -Recurse
Copy-Item "README.md", "LICENSE", ".env.example", ".mcp.json.example" $stage

$zip = "dist/$name.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path $stage -DestinationPath $zip
Write-Host "Created $zip"
