<div align="center">

# PSnippet

**Trình gõ tắt offline cho Windows — gõ mã ngắn, nhấn `Space`, ra nguyên đoạn văn.**

[![CI](https://github.com/phucbtt/PSnippet/actions/workflows/ci.yml/badge.svg)](https://github.com/phucbtt/PSnippet/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/phucbtt/PSnippet?include_prereleases)](https://github.com/phucbtt/PSnippet/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11%20x64-0078D6)

[English](README.md) · Tiếng Việt

</div>

PSnippet hoạt động như bảng gõ tắt của UniKey hay hotstring của AutoHotkey: gõ `ztn` rồi nhấn `Space`, mã được thay ngay tại chỗ bằng
mẫu nhiều dòng. Phù hợp với người phải trả lời những nội dung giống nhau cả ngày (CSKH, hành chính, kinh doanh) và gõ tiếng Việt.
Chạy hoàn toàn offline, không cần quyền admin và **không bao giờ tự gửi tin nhắn**.

> [!NOTE]
> **Trạng thái: giai đoạn đầu (0.1.x).** Đã build và qua unit test, **chưa được kiểm chứng trên Zalo PC thật** — xem
> [bảng tương thích](docs/compatibility-matrix.md). PSnippet là dự án độc lập, không liên kết với hay được xác nhận bởi Zalo / VNG.

## Tính năng

- **Thay thế tức thì:** `mã` + `Space` thay ngay tại chỗ như UniKey. Enter không bao giờ là phím kích hoạt.
- **Biến:** `[Tên]` trong mẫu; palette có form điền biến và xem trước trực tiếp.
- **Dùng ở mọi ứng dụng:** Notepad, Word, trình duyệt, Zalo, Teams, email… Có thể chặn từng ứng dụng, hoặc chỉ cho chạy ở danh sách tự chọn.
- **Phím tắt bật/tắt toàn cục:** `Ctrl+Alt+P`; icon khay chuyển xám khi đang tắt.
- **Ưu tiên tiếng Việt:** Unicode NFC, tương thích Telex/VNI, giao diện tiếng Việt.
- **Nhập / xuất:** định dạng cũ `::mã::( … )return`, JSON `.zsnip`, sao lưu/khôi phục SQLite.
- **Hook bền vững:** hook bàn phím fail-open, tự cài lại sau Win+L/timeout, bỏ qua khi IME đang soạn chữ.
- **Một file duy nhất:** `.exe` liên kết tĩnh, không cần installer hay VC++ Redistributable.

## Cài đặt

1. Tải `PSnippet.exe` (chạy thẳng) hoặc `PSnippet-<phiên bản>-win-x64.zip` (kèm `install.ps1` / `uninstall.ps1`) ở trang
   [Releases](https://github.com/phucbtt/PSnippet/releases).
2. Chạy file. Icon xuất hiện ở khay hệ thống; đóng cửa sổ không dừng engine, chọn **Thoát** ở menu khay mới dừng.

Yêu cầu: Windows 10/11 x64. File **chưa được ký số** nên SmartScreen có thể cảnh báo lần đầu (*More info → Run anyway*).
Có thể đối chiếu với file `.sha256` đi kèm.

## Bắt đầu nhanh

1. Ở bất kỳ ứng dụng nào (Notepad, trình duyệt, cửa sổ chat…) gõ `zok` rồi nhấn `Space` — mã được thay bằng mẫu.
2. Tùy chọn: trong mục **Ứng dụng**, chặn các ứng dụng không muốn gõ tắt, hoặc chuyển sang "Chỉ các ứng dụng tôi chọn".
3. Tạo mẫu riêng ở **Mẫu gõ tắt**, hoặc nhập file kiểu AutoHotkey ở **Nhập / Xuất**.

Dữ liệu lưu tại `%LOCALAPPDATA%\PSnippet\data\snippets.db`. Trong Cài đặt có thể chuyển từ thay thế tức thì sang luồng form.

## Cú pháp mẫu

```text
::ztn::
(
Chào [Tên], em đã nhận được yêu cầu về [Nội dung] và đang kiểm tra ạ.
)
return
```

| Cú pháp | Ý nghĩa |
|---|---|
| `[Tên]` | Biến văn bản (sau khi thay tức thì vẫn giữ `[Tên]` để bạn sửa tay; là ô nhập trong palette) |
| `[Tên:multiline]` | Biến nhiều dòng |
| `[Tên:mention]` / `[Tên:mention?]` | Người cần tag — bắt buộc / tùy chọn |
| `\[` `\]` `\\` | Ký tự `[`, `]`, `\` nguyên văn |

Mã gõ tắt bắt đầu bằng chữ cái, dài 2–32 ký tự (chữ, số, `_`, `-`) và không được trùng (không phân biệt hoa thường).

> **Tag @ trong Zalo:** tag thật chỉ có khi người dùng tự chọn thành viên trong danh sách gợi ý của Zalo. PSnippet không bao giờ giả tag
> từ chữ `@Tên`; luồng mention là thủ công và chưa được kiểm chứng.

## Phím tắt

| Chức năng | Mặc định |
|---|---|
| Bật / tắt gõ tắt | `Ctrl+Alt+P` |
| Mở palette mẫu | `Ctrl+Alt+Space` |
| Tiếp tục sau khi chọn tag | `Ctrl+Alt+Shift+M` |
| Hủy phiên chèn | `Ctrl+Alt+Shift+X` |

Tất cả đổi được trong Cài đặt.

## Riêng tư và an toàn

- Không dùng mạng, không telemetry, không tài khoản; mọi thứ nằm trong một file SQLite cục bộ.
- Không bao giờ gửi tin nhắn; Enter không phải phím kích hoạt.
- Không ghi log nội dung; log mặc định tắt, `ZS_LOG=info` + `ZS_LOG_FILE=<đường dẫn>` chỉ ghi trạng thái.
- Bỏ qua ô mật khẩu, trình quản lý mật khẩu phổ biến và cửa sổ của chính PSnippet; kiểm tra lại cửa sổ đích trước mỗi lần chèn, khôi phục clipboard sau khi dán.
- Báo lỗi bảo mật riêng tư: xem [SECURITY.md](SECURITY.md).

## Build từ mã nguồn

Cần Rust (stable, toolchain MSVC) và *Visual Studio Build Tools* (workload C++ + Windows SDK).

```powershell
cargo test --workspace --locked
cargo build --release --locked        # target\release\psnippet.exe
```

Kiến trúc, lộ trình và quy tắc đóng góp: xem [README tiếng Anh](README.md#architecture), [CONTRIBUTING.md](CONTRIBUTING.md) và
[docs/specs/](docs/specs/).

## Giấy phép

[MIT](LICENSE) © PSnippet contributors
