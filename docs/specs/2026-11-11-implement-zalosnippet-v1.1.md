# 2026-11-11 — Triển khai ZaloSnippet v1.1 (Rust workspace)

## 1. Context / Why

Yêu cầu: triển khai hệ thống theo `docs/ZaloSnippet-Rust-Windows-Full-SPEC-v1.1.md` (text expander offline cho Windows,
hotstring kiểu AHK, form biến động, mention Zalo theo "Segmented Composer", không tự gửi tin nhắn).
Repo ban đầu chỉ có file đặc tả; không có mã nguồn.

## 2. Decision

* Workspace nhiều crate đúng §9; logic thuần tách khỏi Win32 để test được không cần Zalo.
* Mention chỉ ở chế độ **M0 Manual** (đúng §6A.2/§22). `effective_mode()` luôn hạ M1/M2 về Manual vì chưa có
  profile nào đạt QA (`MentionCapability::Unverified`). M1/M2 **chưa triển khai** (cần bằng chứng từ Zalo thật — gate G2/G3).
* FSM mention là **sans-IO** (`Event` → `Action`); không có `Action` nào gửi Enter/Send ⇒ "no autosend" được đảm bảo bởi kiểu dữ liệu.
* Hook callback giữ `TriggerMatcher` trong bộ nhớ, dùng `try_lock` + `try_send` (bounded 64), fail-open khi bận/đầy/panic.
* Chat identity: `chat_signature` luôn `None` cho tới khi có UIA probe riêng cho Zalo (Phase 0 spike chưa làm) ⇒ ngữ cảnh form
  là `Unverifiable`; strict mode ⇒ Copy-only, compat mode ⇒ cho chèn sau cảnh báo. Snippet tĩnh (chạy tức thì sau khi probe
  focus ở thời điểm trigger) được chấp nhận với `Unverifiable`.
* Alternatives bỏ: hotkey toàn cục thay hook cho trigger (không bắt được text), OCR/pixel click (spec cấm), đọc UIA tree toàn desktop (cấm).
* Toolchain: cần MSVC Build Tools (đã cài `Microsoft.VisualStudio.2022.BuildTools` + Windows SDK qua winget trong phiên này).

## 3. Design

| Crate | Trách nhiệm |
|---|---|
| `crates/core` (`zs-core`) | Model `Snippet`/`FieldDefinition`, template language (`[Tên]`, `[Tên:mention?]`, `\[`), `Template::render → Vec<RenderSegment>`, `TriggerMatcher`, `Config` |
| `crates/parser` | Parser legacy `::code::( … )return` (line/col lỗi, BOM, CRLF, `\)`), export legacy, `.zsnip` JSON v1/v2, 7 seed (`examples/default-snippets.ahk.txt`, `include_str!`) |
| `crates/storage` | SQLite (rusqlite bundled): migrations 001+002, CRUD, search, import (Skip/Replace/Rename, transaction), settings, backup (`VACUUM INTO` + rename) / restore (validate + backup API) |
| `crates/mention` | `TargetContext` + `check_context`, `ComposeSession` FSM (§6A.6), `MentionMode`/`effective_mode`, `CandidateConfidence`, `MentionVerification` |
| `crates/platform-windows` | **Toàn bộ `unsafe`**: hook thread (`WH_KEYBOARD_LL`, `WH_MOUSE_LL`, WinEvent foreground, `RegisterHotKey`), UIA worker (COM MTA), clipboard (CF_UNICODETEXT, loại khỏi history/cloud, snapshot + restore theo sequence number), `SendInput` (Backspace/Ctrl+V/Unicode; không có hàm Enter), single-instance, autostart HKCU |
| `crates/ui` | egui views: main window (Dashboard/Snippets/Categories/Import-Export/App Rules/Settings/Diagnostics), form biến + live preview + chip `@Tên · chưa xác nhận`, palette, session panel |
| `crates/app` | Composition root + `ZsApp` coordinator + tray (`tray-icon`) |

Luồng chính (§3.2, §6A.4): hook match → swallow Space → coordinator snapshot context + probe UIA (password/editable) →
(snippet tĩnh) render + FSM, hoặc (có biến) mở form, **trigger vẫn nằm trong ô chat** → confirm → hide popup → `focus_window(target)` →
revalidate (hwnd/pid/process start time/exe/generation/focus signature) → xóa đúng `trigger_len` ký tự → paste từng segment →
tại `Mention`: dừng, chờ người dùng chọn trong Zalo rồi Resume (`Ctrl+Alt+Shift+M`) → revalidate → segment kế tiếp.
Cancel trước confirm: không đụng ô chat. Đổi cửa sổ/timeout: `AbortedPartial`, không rollback, chỉ copy phần chưa chèn.
`generation` tăng mỗi lần foreground đổi, trừ cửa sổ của chính app và lần khôi phục focus do app chủ động (`EXPECT_HWND`).
Khi từ chối trigger (context không an toàn) hook đã nuốt Space ⇒ app phát lại Space/Tab (có marker) để không đổi trải nghiệm gõ.

Cấu hình (`Config`, lưu JSON ở bảng `settings`, key `config`): mặc định đúng §5.3/§5A.3 (Space-only, allowlist rỗng ⇒ gõ tắt tắt ở mọi nơi,
`strict_focus=true`, mention manual, không lưu PII).

## 4. API / Schema changes

Mới hoàn toàn (không có breaking). Schema SQLite = §8 + migration 002 (§8A.1) nguyên văn; `field_definitions.mention_fallback` được dùng,
`field_options_json`, `app_profiles`, `snippet_app_profiles`, `security_preferences` đã tạo nhưng **chưa có code đọc/ghi** (dành cho M1/M2).
`.zsnip` xuất `schema_version: 2`; nhập được v1 (`textarea` → `multiline`).
Biến môi trường chẩn đoán: `ZS_LOG=info|debug`, `ZS_LOG_FILE=<path>` (chỉ trạng thái/mã snippet, không nội dung).

## 5. How to test

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --locked          # 76 unit/contract tests
cargo build --workspace --release --locked
```
Đã chạy trong phiên triển khai: fmt, clippy `-D warnings`, `cargo test --workspace` (core 25, mention 20, parser 12, storage 12, platform 4, ui 3 — đều pass), release build (`target/release/zalosnippet.exe`, ~17 MB).

Phủ test: parser/AST + 7 seed + round-trip (MN-21/35), matcher (word boundary, Enter không bao giờ kích hoạt, literal mode, reset),
FSM với `MockRichEditor` (MN-07/09/10/11/12/27/36/39), `check_context` (PID reuse, đổi chat, thiếu chữ ký), policy hạ mode, storage
(migration, NOCASE duplicate, import policies, backup/restore, 1000 snippet), clipboard UTF-16/CRLF, hotkey parser.

**Chưa kiểm chứng (không thể từ CI/`cargo test`):**
* Không chạy ứng dụng thật: hook bàn phím, tray, egui window, paste vào Notepad/Zalo đều **chưa được quan sát chạy**
  (phiên này bị chặn không cho cài hook/tiêm phím tự động). Script thủ công: `tests/manual/smoke-notepad.ps1`.
* Mọi mục IT-xx/MN-xx cần Zalo PC thật (xem `docs/compatibility-matrix.md`) vẫn `untested`; các release gate G1–G3 chưa đạt.
* Edge cases cần thử tay: UniKey/EVKey (IT-08), DPI 125–200% (IT-12), Win+L (IT-11), clipboard ảnh/file (IT-09), UIPI (IT-10).

**Chưa triển khai (so với spec):** M1 Assisted / M2 Auto (UIA candidate), watchdog hook & hồi phục sau Win+L, phát hiện IME composition
(§3.1 — hiện không chặn trigger khi đang compose), tray "Scope" và "Quick snippets", lưu `remember_field_values`, `proptest`,
crate `integration-tests` riêng (contract test nằm trong `crates/mention`), installer WiX/NSIS, code signing, CI workflow.

**Cập nhật:** watchdog hook, IME guard, `remember_field_values`, tray Scope/Quick snippets, CI đã làm ở `2026-11-11-hook-watchdog-ime-remember-tray.md` (các mục tương ứng trong danh sách "Chưa triển khai" ở trên không còn đúng).
