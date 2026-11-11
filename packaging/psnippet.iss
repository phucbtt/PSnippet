; Optional installer: install Inno Setup 6, then run  iscc packaging\psnippet.iss  (after cargo build --release).
[Setup]
AppName=PSnippet
AppVersion=0.1.0
DefaultDirName={autopf}\PSnippet
PrivilegesRequired=lowest
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=PSnippet-Setup
Compression=lzma2
DefaultGroupName=PSnippet
[Files]
Source: "..\target\release\psnippet.exe"; DestDir: "{app}"
[Icons]
Name: "{group}\PSnippet"; Filename: "{app}\psnippet.exe"
[Run]
Filename: "{app}\psnippet.exe"; Description: "Chạy PSnippet"; Flags: nowait postinstall skipifsilent
