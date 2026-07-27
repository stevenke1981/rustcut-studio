param(
  [string]$Destination = "$env:LOCALAPPDATA\RustCut\bin",
  [switch]$AddToPath
)
$ErrorActionPreference = "Stop"
$Root = Resolve-Path (Join-Path $PSScriptRoot "..")
$Bin = Join-Path $Root "bin"
if (-not (Test-Path (Join-Path $Bin "rustcut-server.exe"))) {
  throw "Run this installer from an extracted release package. rustcut-server.exe was not found."
}
New-Item -ItemType Directory -Force -Path $Destination | Out-Null
Copy-Item (Join-Path $Bin "*.exe") $Destination -Force
if ($AddToPath) {
  $current = [Environment]::GetEnvironmentVariable("Path", "User")
  $parts = @($current -split ';' | Where-Object { $_ })
  if ($parts -notcontains $Destination) {
    [Environment]::SetEnvironmentVariable("Path", (($parts + $Destination) -join ';'), "User")
  }
}
Write-Host "Installed RustCut binaries to $Destination"
Write-Host "Open a new terminal and run: rustcut-server"
