# 2026-11-11 — Đóng gói Windows x64

## 1. Context / Why
Cần bản chạy được trên máy Windows bất kỳ, không phải cài Rust/VC++ Redistributable.

## 2. Decision
* Liên kết tĩnh CRT MSVC (`.cargo/config.toml`, `+crt-static`) ⇒ exe chỉ phụ thuộc DLL có sẵn của Windows
  (kiểm bằng `dumpbin /dependents`: không có vcruntime/msvcp/ucrt).
* Phân phối: zip portable + `install.ps1` (cài theo người dùng, không cần admin) + `uninstall.ps1`; tùy chọn Inno Setup (`zalosnippet.iss`,
  chưa build vì máy chưa có `iscc`). Không ký số (cần chứng chỉ) ⇒ SmartScreen có thể cảnh báo.
* Chỉ x64. ARM64 chưa build (cần `aarch64-pc-windows-msvc`).

## 3. Design
`.cargo/config.toml`; `packaging/{build-dist.ps1,install.ps1,uninstall.ps1,zalosnippet.iss}`; đầu ra `dist/` (gitignored):
`ZaloSnippet-0.1.0-win-x64.zip` (~7 MB) + `.sha256`.

## 4. API / Schema changes
Không. Cấu hình build thay đổi (CRT tĩnh) — không phá vỡ.

## 5. How to test
`powershell -ExecutionPolicy Bypass -File packaging\build-dist.ps1`; giải nén trên máy sạch/VM, chạy `install.ps1`, mở từ Start Menu.
`uninstall.ps1 [-PurgeData]` để gỡ. **Chưa thử trên máy sạch.**
