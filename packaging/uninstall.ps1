# Removes the program and shortcut. User data (%LOCALAPPDATA%\PSnippet) is kept unless -PurgeData is given.
param([switch]$PurgeData)
$dir = Join-Path $env:LOCALAPPDATA "Programs\PSnippet"
Get-Process psnippet -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq (Join-Path $dir "psnippet.exe") } | Stop-Process -Force
Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item (Join-Path ([Environment]::GetFolderPath("Programs")) "PSnippet.lnk") -Force -ErrorAction SilentlyContinue
Remove-ItemProperty "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name "PSnippet" -ErrorAction SilentlyContinue
if ($PurgeData) { Remove-Item (Join-Path $env:LOCALAPPDATA "PSnippet") -Recurse -Force -ErrorAction SilentlyContinue }
