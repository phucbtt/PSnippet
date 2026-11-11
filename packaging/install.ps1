# Per-user install (no admin): copies the exe to %LOCALAPPDATA%\Programs\PSnippet, adds a Start Menu shortcut.
$ErrorActionPreference = "Stop"
$src = Join-Path $PSScriptRoot "psnippet.exe"
if (-not (Test-Path $src)) { throw "psnippet.exe not found next to install.ps1" }
$dir = Join-Path $env:LOCALAPPDATA "Programs\PSnippet"
Get-Process psnippet -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq (Join-Path $dir "psnippet.exe") } | Stop-Process -Force
New-Item -ItemType Directory -Force $dir | Out-Null
Copy-Item $src $dir -Force
$lnk = Join-Path ([Environment]::GetFolderPath("Programs")) "PSnippet.lnk"
$s = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk)
$s.TargetPath = Join-Path $dir "psnippet.exe"; $s.WorkingDirectory = $dir; $s.Save()
Write-Host "Installed to $dir. Start it from the Start Menu (PSnippet). Autostart is a setting inside the app."
# Clean up a pre-rename (ZaloSnippet) install.
Remove-Item (Join-Path $env:LOCALAPPDATA "Programs\ZaloSnippet") -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item (Join-Path ([Environment]::GetFolderPath("Programs")) "ZaloSnippet.lnk") -Force -ErrorAction SilentlyContinue
