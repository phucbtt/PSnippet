# PSnippet

[![CI](../../actions/workflows/ci.yml/badge.svg)](../../actions/workflows/ci.yml) · License: MIT · Windows 10/11 x64 · Rust 2024

**EN** — An offline, open-source text expander for Windows, in the spirit of UniKey's macro table / AutoHotkey hotstrings:
type a short code (e.g. `ztn`) and press `Space`; it is replaced in place with a multi-line (Vietnamese-friendly) template.
Templates can contain `[Variables]`, a palette (`Ctrl+Alt+Space`) with a fill-in form, and a global on/off hotkey (`Ctrl+Alt+P`).
It never sends a message for you, never reads chat content, needs no network and no admin rights.

**VI** — Trình gõ tắt offline, mã nguồn mở cho Windows (giống bảng gõ tắt của UniKey / hotstring của AutoHotkey): gõ mã (ví dụ `ztn`) rồi nhấn `Space`
để thay bằng mẫu nhiều dòng. Hỗ trợ `[Biến]`, palette tìm mẫu (`Ctrl+Alt+Space`), form điền biến, phím tắt bật/tắt (`Ctrl+Alt+P`).
PSnippet **không bao giờ tự gửi tin nhắn**, không đọc nội dung cuộc trò chuyện, không cần mạng hay quyền admin.

> **Status / Trạng thái:** builds, 79+ unit tests pass. **Not yet verified on a real Zalo PC** — see [docs/compatibility-matrix.md](docs/compatibility-matrix.md).
> Zalo "@mention" support is manual-only (the user picks the member in Zalo). PSnippet is an independent project, not affiliated with or endorsed by Zalo / VNG.

## Features

- Hotstrings (`code` + `Space`), word-boundary matching, Enter never triggers; per-app allowlist (empty = disabled everywhere).
- Template language: `[Name]`, `[Name:multiline]`, `[Name:mention]`, escapes `\[ \] \`; legacy `::code::( … )return` import/export and `.zsnip` JSON.
- SQLite storage (bundled), backup/restore, categories, favorites, tray menu, autostart (per-user).
- Safety by design: no auto-send, password fields refused, target-window guard before every injection, clipboard restored, fail-open keyboard hook.
- Single static `.exe` (CRT statically linked), no installer required.

## Build

Requires Rust (stable, MSVC toolchain) and *Visual Studio Build Tools* (C++ workload + Windows SDK).

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
cargo build --release --locked            # target\release\psnippet.exe
powershell -ExecutionPolicy Bypass -File packaging\build-dist.ps1   # dist\PSnippet-<ver>-win-x64.zip
```

## Quick start

1. Run `psnippet.exe` (tray icon; closing the window keeps the engine running; "Thoát/Exit" in the tray menu stops it).
2. **Ứng dụng / App Rules → add the current app** (click the target window within 3 s). No app listed ⇒ hotstrings are off everywhere.
3. In that app type `zok` + `Space`. Settings lets you switch to the form flow instead of instant expansion.

Data lives in `%LOCALAPPDATA%\PSnippet\data\snippets.db`. Logging is off by default; set `ZS_LOG=info` and `ZS_LOG_FILE=<path>` for a state-only trace (never content).

## Repository layout

`crates/core` (model, template, matcher) · `parser` (legacy + JSON) · `storage` (SQLite) · `mention` (session FSM, guards) ·
`platform-windows` (**all `unsafe`**: hooks, UIA, clipboard, SendInput) · `ui` (egui) · `app` (tray, coordinator, logo, `psnippet.exe`).
Design spec: [docs/ZaloSnippet-Rust-Windows-Full-SPEC-v1.1.md](docs/ZaloSnippet-Rust-Windows-Full-SPEC-v1.1.md) (original name) ·
change records: [docs/specs/](docs/specs/).

## Contributing & security

See [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md). Licensed under the [MIT License](LICENSE).
