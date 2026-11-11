# 2026-11-11 — Gõ tắt kiểu UniKey (instant expand)

## 1. Context / Why
Người dùng muốn hành vi như bảng gõ tắt của UniKey: gõ mã + Space ⇒ thay ngay tại chỗ, không form, không bước xác nhận.

## 2. Decision
`Config.instant_expand` (mặc định `true`). Trigger gõ tay ⇒ xóa mã (Backspace đúng số ký tự) và dán nội dung ngay. Snippet có biến: biến giữ
nguyên dạng literal `[Tên]` để người dùng sửa tại chỗ (không tạo mention giả, không dừng phiên). Palette (`Ctrl+Alt+Space`) vẫn mở form.
`false` ⇒ hành vi form cũ. Ở chế độ instant, yêu cầu "ô nhập có thể chỉnh sửa" của strict bị bỏ qua (Zalo/Electron thường không expose UIA);
chỉ từ chối ô mật khẩu. Các guard còn lại giữ nguyên (cửa sổ/PID/generation, allowlist, không bao giờ gửi tin).

## 3. Design
`core/template.rs`: `Template::render_placeholders()`. `core/config.rs`: `instant_expand`. `app/orchestrator.rs`: `begin_compose` coi
trigger gõ tay là `immediate`; `evaluate_probe` nới yêu cầu editable. `ui/main_view.rs`: checkbox trong Settings.

## 4. API / Schema changes
Config thêm `instant_expand` (serde default `true`; thay đổi hành vi mặc định cho snippet có biến — không phá vỡ dữ liệu).

## 5. How to test
`cargo test --workspace` (thêm `placeholders_keep_fields_as_literal_brackets`). Thủ công: thêm app vào App Rules, gõ `ztn` + Space ⇒ thay ngay;
mã có `[Tên]` ⇒ ra `[Tên]`. Settings → bỏ tick để quay lại form. **Chưa thử trên Zalo/Notepad thật.**
