# 2026-11-11 — About + Releases

## 1. Context / Why
Repo GitHub cần mục About (mô tả/topics) và Releases (tải bản chạy); app cần trang giới thiệu có phiên bản và liên kết.

## 2. Decision
Release tạo tự động bằng GitHub Actions khi push tag `v*` (dùng `GITHUB_TOKEN`, không cần lưu token trên máy). About của repo (mô tả, topics) chỉ đặt được qua UI/API GitHub — làm tay.

## 3. Design
`.github/workflows/release.yml`: test → `packaging/build-dist.ps1` → đính kèm `PSnippet.exe`, `.sha256`, zip. `ui/main_view.rs`: `Page::About` ("Giới thiệu": phiên bản từ `CARGO_PKG_VERSION`, liên kết GitHub/Releases/Issues, giấy phép, disclaimer).

## 4. API / Schema changes
Không.

## 5. How to test
Push tag `v0.1.0` ⇒ tab Actions chạy `release`, Release xuất hiện kèm 4 file. Trong app: mục "Giới thiệu" hiện phiên bản và các liên kết mở được.
