# 2026-11-11 — Logo PSnippet

## 1. Context / Why
Icon khay vẽ chữ "Z" (từ tên ZaloSnippet) nên không khớp tên PSnippet; .exe và cửa sổ không có icon riêng.

## 2. Decision
Logo "P" trắng trên nền vuông bo góc xanh (màu ACCENT), vẽ bằng code (không asset nhị phân, không phụ thuộc font). Một nguồn duy nhất cho khay, cửa sổ và .exe.

## 3. Design
`crates/app/src/logo.rs`: `rgba(n, bg)` (supersample 4×4), `ico(sizes)` (BMP 32-bit, 16–256 px). `build.rs` sinh `psnippet.ico` rồi nhúng bằng `winresource`
(ProductName/FileDescription kèm theo; thiếu rc.exe chỉ cảnh báo, không lỗi build). `tray.rs` dùng `logo::rgba` (xám khi tạm dừng); `main.rs` đặt icon cửa sổ 64 px.

## 4. API / Schema changes
Không. Thêm build-dependency `winresource`.

## 5. How to test
`cargo test -p psnippet` (2 test logo). Trích icon từ `psnippet.exe` (System.Drawing) — đã kiểm tra thấy chữ P. Thủ công: kiểm tra icon khay (xanh/xám), thanh tiêu đề, Explorer.
