# Builds target\release and produces dist\PSnippet-<version>-win-x64.zip (exe + install/uninstall scripts + README).
$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot)
cargo build --release --locked
if ($LASTEXITCODE) { throw "build failed" }
$ver = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"(.+)"' | Select-Object -First 1).Matches.Groups[1].Value
if (-not $ver) { $ver = "0.1.0" }
$stage = "dist\PSnippet-$ver-win-x64"
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item target\release\psnippet.exe, packaging\install.ps1, packaging\uninstall.ps1, README.md $stage
Compress-Archive -Path "$stage\*" -DestinationPath "$stage.zip" -Force
(Get-FileHash "$stage.zip" -Algorithm SHA256).Hash + "  " + (Split-Path "$stage.zip" -Leaf) | Set-Content "$stage.zip.sha256"
Write-Host "Created $stage.zip"
