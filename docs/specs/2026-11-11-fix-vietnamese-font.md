# 2026-11-11 — Sửa lỗi font tiếng Việt

## 1. Context / Why
UI hiển thị sai/thiếu dấu tiếng Việt: font mặc định của egui không phủ đủ chữ có dấu.

## 2. Decision
`zs_ui::install_fonts` đã tồn tại nhưng không bao giờ được gọi. Gọi nó ở `ZsApp::new`. Font lấy từ máy người dùng
(`%WINDIR%\Fonts`: Segoe UI → Tahoma → Arial), không đóng gói kèm (giấy phép font).

## 3. Design
`crates/ui/src/lib.rs`: dùng `%WINDIR%`, thêm Tahoma, thêm làm fallback cho Monospace. `crates/app/src/orchestrator.rs`: gọi `install_fonts` trước khi tạo tray.

## 4. API / Schema changes
Không.

## 5. How to test
Mở app, kiểm tra chữ có dấu (ế, ộ, ữ, ặ) ở Snippets/Settings/form. **Chưa xác minh bằng mắt trong phiên này.**
