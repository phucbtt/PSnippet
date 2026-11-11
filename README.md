<div align="center">

# PSnippet

**Offline text expander for Windows — type a short code, press `Space`, get the full text.**

[![CI](https://github.com/phucbtt/PSnippet/actions/workflows/ci.yml/badge.svg)](https://github.com/phucbtt/PSnippet/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/phucbtt/PSnippet?include_prereleases)](https://github.com/phucbtt/PSnippet/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11%20x64-0078D6)
![Rust](https://img.shields.io/badge/Rust-2024-orange)

English · [Tiếng Việt](README.vi.md)

</div>

PSnippet works like UniKey's macro table or AutoHotkey hotstrings: type `ztn` and press `Space`, and the code is replaced in place
with a multi-line template. It is built for people who answer the same messages all day (support teams, admins, sales) and who
type Vietnamese. It runs fully offline, needs no administrator rights, and **never sends a message for you**.

> [!NOTE]
> **Project status: early (0.1.x).** It builds, passes its unit tests, and has not yet been verified on a real Zalo PC client —
> see the [compatibility matrix](docs/compatibility-matrix.md). PSnippet is an independent project and is not affiliated with or
> endorsed by Zalo / VNG.

## Contents

[Features](#features) · [Install](#install) · [Quick start](#quick-start) · [Template syntax](#template-syntax) ·
[Hotkeys](#hotkeys) · [Privacy and safety](#privacy-and-safety) · [Build from source](#build-from-source) ·
[Architecture](#architecture) · [Roadmap](#roadmap) · [Contributing](#contributing) · [License](#license)

## Features

| | |
|---|---|
| **Instant expansion** | `code` + `Space` is replaced in place, UniKey-style. Enter never triggers. Word-boundary aware. |
| **Variables** | `[Name]` placeholders in templates; a palette with a fill-in form and live preview when you prefer typing values first. |
| **Per-app allowlist** | Hotstrings only fire in apps you add (empty list = disabled everywhere). Optional "Zalo only" scope from the tray. |
| **Global on/off hotkey** | `Ctrl+Alt+P` pauses/resumes expansion; the tray icon turns grey while paused. |
| **Vietnamese first** | Unicode (NFC) throughout, Telex/VNI-friendly typing window, Vietnamese UI. |
| **Import / export** | Legacy `::code::( … )return` blocks, `.zsnip` JSON, SQLite backup and restore. |
| **Robust hook** | Fail-open keyboard hook, watchdog that reinstalls it after Win+L / timeouts, IME-composition guard. |
| **Single file** | One static `.exe` (C runtime linked in). No installer required, no VC++ redistributable. |

## Install

1. Download `PSnippet.exe` (portable) or `PSnippet-<version>-win-x64.zip` (adds `install.ps1` / `uninstall.ps1`) from the
   [Releases](https://github.com/phucbtt/PSnippet/releases) page.
2. Run it. A tray icon appears; closing the window keeps the engine running, **Exit** in the tray menu stops it.

Requirements: Windows 10 or 11, x64. The binary is **not code-signed yet**, so SmartScreen may warn on first run
(*More info → Run anyway*). Verify the download against the published `.sha256` if in doubt.

## Quick start

1. Open **Ứng dụng (App Rules)** and add the application you type into (click its window within 3 seconds).
2. In that app, type `zok` and press `Space` — the code is replaced by its template.
3. Add your own templates under **Mẫu gõ tắt (Snippets)**, or import an existing AutoHotkey-style file under **Nhập / Xuất**.

Data is stored in `%LOCALAPPDATA%\PSnippet\data\snippets.db`. Settings let you switch from instant expansion to the form flow.

## Template syntax

```text
::ztn::
(
Hello [Name], we have received your request about [Topic] and are checking it now.
)
return
```

| Syntax | Meaning |
|---|---|
| `[Name]` | Text placeholder (left as `[Name]` for you to overwrite after instant expansion; a form field in the palette) |
| `[Name:multiline]` | Multi-line text field |
| `[Name:mention]` / `[Name:mention?]` | Person to tag — required / optional (see the Zalo note below) |
| `\[` `\]` `\\` | Literal `[`, `]`, `\` |

The same variable name used twice is filled once. A code starts with a letter, is 2–32 characters
(letters, digits, `_`, `-`) and must be unique (case-insensitive).

> **Zalo @mentions.** A real mention exists only when the user picks the member from Zalo's own suggestion list. PSnippet never
> fakes a tag from `@Name` text; the mention workflow is manual and currently unverified.

## Hotkeys

| Action | Default | Configurable |
|---|---|---|
| Turn expansion on / off | `Ctrl+Alt+P` | Settings |
| Open the snippet palette | `Ctrl+Alt+Space` | Settings |
| Continue after choosing a mention | `Ctrl+Alt+Shift+M` | Settings |
| Cancel a running insertion | `Ctrl+Alt+Shift+X` | Settings |

## Privacy and safety

- **No network, no telemetry, no accounts.** Everything stays in a local SQLite file.
- **Never sends a message.** There is no code path that presses Enter/Send; Enter is never a trigger.
- **No content logging.** Logs are off by default; `ZS_LOG=info` with `ZS_LOG_FILE=<path>` writes a state-only trace (no typed text, no chat content).
- **Guarded insertion.** Password fields are refused; the target window is re-validated before every injection; the clipboard is restored after pasting.
- **Fail-open keyboard hook.** If anything is uncertain the keystroke passes through untouched.
- Report vulnerabilities privately — see [SECURITY.md](SECURITY.md).

## Build from source

Requires Rust (stable, MSVC toolchain) and *Visual Studio Build Tools* (C++ workload + Windows SDK).

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
cargo build --release --locked                                       # target\release\psnippet.exe
powershell -ExecutionPolicy Bypass -File packaging\build-dist.ps1    # dist\PSnippet-<ver>-win-x64.zip
```

Releases are built by GitHub Actions when a `v*` tag is pushed ([release.yml](.github/workflows/release.yml)).

## Architecture

| Crate | Responsibility |
|---|---|
| `crates/core` | Snippet model, template language, trigger matcher, configuration |
| `crates/parser` | Legacy and JSON import/export, seed snippets |
| `crates/storage` | SQLite persistence, migrations, backup/restore |
| `crates/mention` | Insertion-session state machine and target-context guards (no send action exists) |
| `crates/platform-windows` | **All `unsafe` code**: hooks, UI Automation, clipboard, `SendInput`, autostart |
| `crates/ui` | egui views: main window, form, palette |
| `crates/app` | `psnippet.exe`: tray, coordinator, logo |

Design notes: [original specification](docs/ZaloSnippet-Rust-Windows-Full-SPEC-v1.1.md) (written under the project's former name) and
the per-change records in [docs/specs/](docs/specs/).

## Roadmap

- [ ] Verification on real Zalo PC builds (Phase 0 spike) and compatibility-matrix updates
- [ ] Assisted / automatic mention modes (only with device evidence)
- [ ] Code signing and an installer
- [ ] ARM64 build
- [ ] Property-based tests for the template parser and matcher

## Contributing

Issues and pull requests are welcome — read [CONTRIBUTING.md](CONTRIBUTING.md) first (it lists the product-safety rules).
Changes are tracked in [CHANGELOG.md](CHANGELOG.md).

## License

[MIT](LICENSE) © PSnippet contributors
