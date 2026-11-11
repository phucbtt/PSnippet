# 2026-11-11 — Chuẩn bị mã nguồn mở (GitHub)

## 1. Context / Why
Cần đóng gói source để công khai trên GitHub.

## 2. Decision
Giấy phép MIT (đã khai trong `Cargo.toml`), chủ bản quyền "PSnippet contributors" (có thể đổi thành tên/tổ chức thật trước khi công khai).
README song ngữ EN/VI kèm disclaimer không liên kết với Zalo/VNG. Không tự `git init`/commit/push (hành động công khai do chủ repo quyết định).

## 3. Design
Thêm `LICENSE`, `README.md` (viết lại), `CONTRIBUTING.md`, `SECURITY.md`, `CHANGELOG.md`, issue/PR template, `.gitignore` (target, dist, *.db, .claude, .vscode).
Gói nguồn `dist/psnippet-0.1.0-source.zip` (không có target/dist/.git); đã giải nén ra thư mục sạch và chạy `cargo test --workspace --locked` thành công.
Rà soát: không có tên/email/đường dẫn cá nhân trong file nguồn; 7 mẫu seed là văn bản CSKH chung, không chứa dữ liệu khách hàng.

## 4. API / Schema changes
Không.

## 5. How to test
Giải nén zip → `cargo test --workspace --locked`; `cargo build --release --locked`.
