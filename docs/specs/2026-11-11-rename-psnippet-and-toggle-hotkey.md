# 2026-11-11 — Đổi tên PSnippet + phím tắt bật/tắt gõ tắt

## 1. Context / Why
Yêu cầu: đổi tên sản phẩm thành PSnippet; thêm phím tắt bật/tắt chức năng gõ tắt.

## 2. Decision
* Đổi tên hiển thị, crate/binary (`psnippet.exe`), thư mục dữ liệu `%LOCALAPPDATA%\PSnippet`, mutex single-instance, giá trị autostart, định dạng export (`psnippet.export`).
* Tương thích ngược: tự đổi tên thư mục dữ liệu cũ `ZaloSnippet` → `PSnippet` (nếu bị khóa vì bản cũ đang chạy thì tiếp tục dùng thư mục cũ);
  import vẫn nhận `zalosnippet.export`; xóa giá trị autostart cũ `ZaloSnippet`; `install.ps1` dọn bản cài cũ.
* Giữ nguyên tên file đặc tả gốc và các spec đã ghi (bản ghi lịch sử).
* Phím tắt bật/tắt: `Config.toggle_hotkey` (mặc định `Ctrl+Alt+P`), `HotkeyAction::TogglePause` (RegisterHotKey id 4) — cùng hiệu ứng với Tạm dừng ở khay,
  hiện thông báo tooltip BẬT/TẮT. Đổi được trong Settings → Phím tắt toàn cục; xung đột phím hiện ở Diagnostics.

## 3. Design
`platform-windows/hook.rs` (+`TogglePause`), `core/config.rs` (+`toggle_hotkey`), `app/orchestrator.rs` (đăng ký, xử lý, tuple hotkeys 4 phần tử),
`ui/main_view.rs` (hàng cấu hình), `parser/json.rs` (`FORMAT`, `LEGACY_FORMAT`), `app/main.rs` (migrate data dir), `platform-windows/system.rs`.

## 4. API / Schema changes
Config thêm `toggle_hotkey` (serde default). Export mới ghi `psnippet.export` (bản cũ trước đổi tên vẫn import được; bản app cũ không import được file mới). Không đổi schema SQLite.

## 5. How to test
`cargo test --workspace`. Thủ công: chạy `PSnippet.exe`, nhấn `Ctrl+Alt+P` ⇒ icon khay xám + tooltip "ĐÃ TẮT"; gõ mã + Space không mở rộng; nhấn lại ⇒ bật.
Kiểm tra dữ liệu cũ vẫn còn sau khi đóng bản ZaloSnippet cũ. **Chưa thử trên máy thật.**
