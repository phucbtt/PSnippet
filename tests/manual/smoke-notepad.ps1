# MANUAL smoke test (NOT run by CI). Run it yourself in an interactive desktop session.
# It starts PSnippet with an isolated data folder (LOCALAPPDATA redirected to a temp dir),
# allow-lists notepad.exe, then types "hello zhi " into a Notepad window it launched itself and
# reads the result back through the clipboard.
#
# Side effects: installs the app's global keyboard hook while running, sends keystrokes via
# SendKeys, overwrites your clipboard. It only stops the two processes it started.
param([string]$Exe = "$PSScriptRoot\..\..\target\debug\psnippet.exe")
$ErrorActionPreference = "Stop"
$root = Join-Path $env:TEMP ("zs-smoke-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory $root | Out-Null
$env:LOCALAPPDATA = $root; $env:ZS_LOG = "info"; $env:ZS_LOG_FILE = Join-Path $root "app.log"

$p = Start-Process $Exe -ArgumentList "--background" -PassThru   # 1st run: create + seed DB
Start-Sleep 4; Stop-Process -Id $p.Id -Force; Start-Sleep 1

$py = @'
import sqlite3, json, uuid, sys
c = sqlite3.connect(sys.argv[1])
c.execute("INSERT OR REPLACE INTO settings(key,value_json) VALUES('config',?)", (json.dumps({"allowed_apps": ["notepad.exe"]}),))
c.execute("INSERT INTO snippets(id,code,title,body,created_at,updated_at) VALUES(?,?,?,?,?,?)",
  (str(uuid.uuid4()),"zhi","static","Xin chào, đây là mẫu tĩnh.\nDòng hai: ăn cơm chưa?","2026-11-11T00:00:00Z","2026-11-11T00:00:00Z"))
c.commit()
'@
$pyFile = Join-Path $root "cfg.py"; Set-Content $pyFile $py -Encoding utf8
python -I $pyFile (Join-Path $root "PSnippet\data\snippets.db")

$app = Start-Process $Exe -ArgumentList "--background" -PassThru
Start-Sleep 3
$np = Start-Process notepad -PassThru; Start-Sleep 3
Add-Type -AssemblyName Microsoft.VisualBasic, System.Windows.Forms
[Microsoft.VisualBasic.Interaction]::AppActivate($np.Id); Start-Sleep 1
[System.Windows.Forms.SendKeys]::SendWait("hello zhi "); Start-Sleep 3
[System.Windows.Forms.SendKeys]::SendWait("^a^c"); Start-Sleep 1
"Notepad text: [" + (Get-Clipboard -Raw) + "]"
"---- app log ----"; Get-Content $env:ZS_LOG_FILE -ErrorAction SilentlyContinue
Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
Stop-Process -Id $np.Id -Force -ErrorAction SilentlyContinue
Write-Host "Expected: 'hello Xin chào, đây là mẫu tĩnh.' + line 2. If the log says E_FOCUS_UNKNOWN, Notepad's"
Write-Host "UIA focus was not Edit/Document under strict mode: set strict_focus=false in the config row and retry."
