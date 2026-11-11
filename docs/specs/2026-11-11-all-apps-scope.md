# 2026-11-11 — Gõ tắt dùng được ở mọi ứng dụng

## 1. Context / Why
PSnippet không chỉ dành cho Zalo. Trước đây danh sách ứng dụng cho phép mặc định rỗng ⇒ gõ tắt tắt ở mọi nơi cho tới khi người dùng thêm
từng ứng dụng; menu khay còn mục "Phạm vi: chỉ Zalo".

## 2. Decision
Mặc định **mọi ứng dụng, trừ danh sách chặn** (giống UniKey). Vẫn giữ chế độ "Chỉ các ứng dụng tôi chọn" cho ai cần giới hạn.
Danh sách chặn luôn thắng. Luôn loại trừ: ô mật khẩu (UIA), cửa sổ của chính PSnippet (theo PID), ứng dụng không xác định được
tiến trình (ví dụ chạy quyền admin — fail-closed). Danh sách chặn mặc định: KeePass, KeePassXC, 1Password, Bitwarden.
Đây là thay đổi so với đặc tả gốc (§5.3: allowlist rỗng ⇒ fail-closed) theo yêu cầu người dùng.

## 3. Design
* `core/config.rs`: `allow_all_apps` (mặc định `true`), `blocked_apps`, `AppScope { all, allowed, blocked }` + `permits()`,
  `app_entry_matches()` (chuyển từ `platform-windows/context.rs`, giờ chỉ là wrapper).
* `platform-windows/hook.rs`: `HookConfig.scope`, `Shared::set_app_scope`; `is_allowed_window` bỏ qua PID của chính app.
* `app/orchestrator.rs`: dùng `app_scope()`; "Thêm ứng dụng đang dùng" thêm vào danh sách chặn (chế độ mọi ứng dụng) hoặc danh sách cho phép.
* `app/tray.rs`: "Phạm vi: chỉ Zalo" → "Chỉ chạy ở ứng dụng đã chọn".
* `ui/main_view.rs`: trang Ứng dụng có lựa chọn chế độ + danh sách tương ứng; Tổng quan chỉ cảnh báo khi ở chế độ chọn lọc mà danh sách rỗng.
* README EN/VI cập nhật.

## 4. API / Schema changes
Config thêm `allow_all_apps`, `blocked_apps` (serde default). **Thay đổi hành vi:** cấu hình cũ không có `allow_all_apps` ⇒ được hiểu là `true`,
nên người dùng hiện tại sẽ có gõ tắt ở mọi ứng dụng sau khi cập nhật. `Shared::set_allowed_apps` → `set_app_scope`; `HookConfig.allowed_apps` → `scope`.
`zalo_only` vẫn đọc được nhưng chỉ áp dụng cho chế độ chọn lọc và không còn trên giao diện.

## 5. How to test
`cargo test --workspace` (thêm `app_scope_all_apps_except_blocked`, `app_scope_allowlist_mode`). Thủ công: cài mới ⇒ gõ `zok`+Space trong Notepad,
Word, trình duyệt đều được thay; mở KeePassXC ⇒ không kích hoạt; ô sửa mẫu trong PSnippet ⇒ không kích hoạt; chặn Notepad ⇒ không kích hoạt;
khay → "Chỉ chạy ở ứng dụng đã chọn" với danh sách rỗng ⇒ tắt ở mọi nơi. **Chưa thử trên máy thật.**
