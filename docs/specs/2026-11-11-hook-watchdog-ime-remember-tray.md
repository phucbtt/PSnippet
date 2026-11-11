# 2026-11-11 — Hook watchdog, IME guard, remembered form values, tray Scope/Quick snippets, CI

## 1. Context / Why
Follow-up to `2026-11-11-implement-zalosnippet-v1.1.md`, which listed these as not implemented: hook watchdog / Win+L
recovery, IME composition handling (§3.1), `remember_field_values`, tray "Scope" + "Quick snippets" (§5.2), CI workflow.

## 2. Decision
* **Watchdog without extra APIs:** every low-level hook callback stamps `GetTickCount`; a 2 s `WM_TIMER` on the hook thread
  compares with `GetLastInputInfo`. Input newer than the last stamp by > 3 s ⇒ hooks were dropped by Windows (callback
  timeout, secure-desktop switch) ⇒ unhook + reinstall + reset matcher. Alternative (WTS session notifications) needs a
  window and misses timeout-removal; rejected.
* **IME:** a key with `VK_PROCESSKEY` means an IME is composing; the matcher window is dropped (never swallowed). UniKey/EVKey
  (hook-based, not IMEs) keep working through the normal path.
* **Remembered values:** opt-in (`remember_field_values`, default off). Stored in `settings` as `fieldvals:<snippet id>` →
  {field key → text}. Mention fields (person names) are never stored. Turning the option off deletes all `fieldvals:*` rows.
* **Scope:** `Config.zalo_only`; hook enforces `effective_allowed_apps()` (allowlist entries whose *file name* contains
  "zalo"). Default `false` ⇒ previous behaviour. Never widens the allowlist.
* **Quick snippets:** tray submenu of up to 10 favorites. Static snippet ⇒ copy to clipboard; snippet with fields ⇒ copy-only
  form (a tray click leaves no verified target). Nothing is typed or sent from the tray.
* **Not done (needs external tooling/evidence):** installer/signing, M1/M2 mention, proptest, live Zalo QA.

## 3. Design
| File | Change |
|---|---|
| `platform-windows/src/hook.rs` | `Shared.{last_hook_tick,reinstalls,ime_keys}`, `install_keyboard/mouse`, `is_stale`, `hooks_look_dead`, timer in message loop, `VK_PROCESSKEY` branch in `handle_key` |
| `platform-windows/Cargo.toml` | feature `Win32_System_SystemInformation` |
| `core/src/config.rs` | `zalo_only`, `effective_allowed_apps()`, `is_zalo_entry()` |
| `storage/src/lib.rs` | `delete_settings_with_prefix()` (LIKE-escaped) |
| `app/src/orchestrator.rs` | prefill/remember values, `refresh_quick`, `on_quick_snippet`, scope toggle, diagnostics counters |
| `app/src/tray.rs` | "Mẫu nhanh" submenu, "Phạm vi: chỉ Zalo" check item |
| `ui/src/main_view.rs` | `EngineStatus.{hook_reinstalls,ime_keys}` shown in Diagnostics |
| `.github/workflows/ci.yml` | fmt, clippy `-D warnings`, test, release build on windows-latest |

## 4. API / Schema changes
Non-breaking. `Config` gains `zalo_only` (serde default `false`; old JSON loads). `Store::delete_settings_with_prefix` added.
`Tray::new(autostart, zalo_only)`. New settings keys `fieldvals:*`. No SQL schema change.

## 5. How to test
* `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo test --workspace --locked`
  (79 tests: new `watchdog_staleness`, `zalo_only_scope_filters_allowlist`, `delete_settings_by_prefix_is_literal`).
* **Manual (not run in this session):** (1) Win+L, unlock, type a trigger in an allowed app ⇒ Diagnostics "Hook phải cài lại"
  increments and the trigger works. (2) Telex via Windows IME: Diagnostics IME counter rises, no false trigger.
  (3) Settings ⇒ remember values on; confirm a form; reopen ⇒ prefilled, mention field empty; turn off ⇒ values gone.
  (4) Tray ⇒ favorite snippet appears under "Mẫu nhanh"; "Phạm vi: chỉ Zalo" disables hotstrings in Notepad.
