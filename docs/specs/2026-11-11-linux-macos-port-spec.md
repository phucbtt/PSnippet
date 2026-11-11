# 2026-11-11 — Đặc tả triển khai PSnippet cho Linux và macOS

> **Trạng thái:** đặc tả thiết kế, **chưa triển khai**. Mọi hành vi phụ thuộc hệ điều hành / bộ gõ / ứng dụng đích ghi ở đây là giả định
> cần kiểm chứng ở Phase 0 trên máy thật; không được đánh dấu "đã hỗ trợ" trước khi có bằng chứng (cùng nguyên tắc với
> [compatibility-matrix.md](../compatibility-matrix.md)).

## 1. Context / Why

PSnippet hiện chỉ chạy trên Windows: toàn bộ phần phụ thuộc hệ điều hành nằm trong `crates/platform-windows`, nhưng `crates/app`
gọi trực tiếp các hàm của crate này (`pw::context::*`, `pw::inject::*`, `pw::clipboard::*`, `pw::system::*`, `hook::HookHandle`) và
`TargetContext` dùng khái niệm riêng của Win32 (`hwnd: isize`, đường dẫn `.exe`). Cần mở rộng sang macOS và Linux mà:

* giữ nguyên các bảo đảm an toàn của sản phẩm: **không bao giờ tự gửi tin**, không đọc nội dung chat, không ghi log nội dung gõ,
  từ chối ô mật khẩu, kiểm tra lại cửa sổ đích trước mỗi lần chèn, fail-open khi bắt phím;
* dùng chung `core`, `parser`, `storage`, `mention`, `ui` (đã thuần Rust, không phụ thuộc OS);
* chấp nhận rằng mỗi nền tảng có giới hạn khác nhau và **fail-closed** khi không xác minh được ngữ cảnh.

Phạm vi người dùng: macOS có Zalo cho Mac và trình duyệt; Linux không có Zalo PC chính thức nên mục tiêu là Zalo Web trong trình duyệt
và các ứng dụng chat/soạn thảo nói chung.

## 2. Decision

| Chủ đề | Quyết định | Phương án loại bỏ / lý do |
|---|---|---|
| Kiến trúc | Thêm crate `zs-platform` (trait + kiểu dữ liệu trung lập), ba adapter `platform-windows`, `platform-macos`, `platform-linux`; `app` chỉ phụ thuộc trait. | `#[cfg]` rải trong `orchestrator.rs` — khó test, dễ lọt API Win32. |
| macOS bắt phím | `CGEventTap` (session tap, head insert) — nuốt được phím delimiter như Windows. | `NSEvent.addGlobalMonitor` chỉ quan sát, không nuốt được. |
| Linux bắt phím | Đọc **evdev** (`/dev/input/event*`) ở chế độ thụ động — chạy được trên cả X11 và Wayland. | XRecord/XInput2: chỉ X11. `EVIOCGRAB` + phát lại qua uinput: nuốt được phím nhưng có thể làm "chết" bàn phím khi app lỗi — loại bỏ ở v1. |
| Linux thay thế | **Post-delimiter replacement**: không nuốt Space; sau khi Space đã vào ô nhập, xóa `trigger_len + 1` ký tự rồi chèn (tùy chọn gõ lại Space). | Nuốt Space trên Linux cần grab toàn bộ thiết bị (xem trên). |
| Linux chèn | X11: XTest. Wayland: thiết bị ảo **uinput**. Clipboard: X11 selection / Wayland `wlr-data-control` hoặc `ext-data-control` qua crate `arboard`. | Portal RemoteDesktop: mỗi lần chạy hỏi quyền — để giai đoạn sau. |
| Nhận diện ứng dụng & ô nhập | macOS: `NSWorkspace` + Accessibility (AX). Linux: **AT-SPI2** (D-Bus) cho cả X11 và Wayland; X11 bổ sung `_NET_ACTIVE_WINDOW`. | Wayland không có API chuẩn cho "cửa sổ đang focus". |
| Wayland không xác định được ứng dụng | **Tắt gõ tắt** (fail-closed). Người dùng có thể bật "Mọi ứng dụng" kèm cảnh báo rõ. | Âm thầm cho chạy mọi nơi — vi phạm nguyên tắc allowlist. |
| Đóng gói | macOS: `.app` universal (x86_64 + arm64), ký Developer ID + notarize, DMG. Linux: AppImage + `.deb` (kèm udev rule cho uinput). | Flatpak/Snap: sandbox chặn `/dev/input`, `/dev/uinput` — không phù hợp v1. |

## 3. Design

### 3.1. Tách lớp nền tảng

```
crates/
  platform/            (zs-platform)  traits + kiểu trung lập, KHÔNG unsafe
  platform-windows/    impl hiện có, chuyển sang implement trait
  platform-macos/      #![cfg(target_os = "macos")]   — toàn bộ unsafe/FFI macOS
  platform-linux/      #![cfg(target_os = "linux")]   — toàn bộ unsafe/FFI Linux
```

Trait đề xuất (đồng bộ hoặc qua worker thread như Windows hiện tại):

```rust
pub trait KeyboardSource {            // hook/tap/evdev trên một thread riêng
    fn start(cfg: HookConfig) -> Result<(Self, Receiver<HookEvent>)> where Self: Sized;
    fn shared(&self) -> &dyn HookControl; // set_paused, set_codes, set_allowed_apps, counters…
    fn swallows_delimiter(&self) -> bool; // Windows/macOS: true; Linux: false
}
pub trait Focus {
    fn snapshot(&self, timeout: Duration) -> Option<(TargetContext, Option<FocusProbe>)>;
    fn focus_window(&self, id: WindowId) -> bool;   // có thể trả false (Wayland) ⇒ không chèn
    fn generation(&self) -> u64;
    fn current_app(&self) -> Option<AppId>;
}
pub trait Injector {                  // KHÔNG có hàm Enter/Send — giữ bất biến "no autosend" bằng kiểu
    fn backspaces(&self, n: usize) -> Result<()>;
    fn paste(&self, text: &str, chord: PasteChord) -> Result<()>;
    fn type_space(&self) -> Result<()>;
}
pub trait Clipboard { fn set_text(&self, t: &str) -> Result<Token>; fn snapshot(&self) -> Result<Snapshot>; fn restore_if_unchanged(&self, s: &Snapshot, t: Token) -> bool; }
pub trait System { fn single_instance(&self) -> Option<InstanceGuard>; fn set_autostart(&self, on: bool) -> Result<()>; fn data_dir(&self) -> PathBuf; fn ui_font_paths(&self) -> Vec<PathBuf>; }
```

`TargetContext` (crate `mention`) đổi sang kiểu trung lập:

| Trường hiện tại | Trường mới | Windows | macOS | Linux |
|---|---|---|---|---|
| `hwnd: isize` | `window: WindowId(u64)` | HWND | `CGWindowID` (hoặc 0 nếu không có quyền Screen Recording) | X11 window id; Wayland: 0 |
| `pid`, `process_start_time` | giữ | như cũ | `proc_pidinfo` start time | `/proc/<pid>/stat` trường 22 |
| `executable_path: String` | `app: AppId` | đường dẫn `.exe` | bundle id (`CFBundleIdentifier`) | đường dẫn `/proc/<pid>/exe`; Wayland/AT-SPI: tên ứng dụng AT-SPI |
| `focus_signature` | giữ | UIA RuntimeId | AX element (hash của role + frame + pid) | AT-SPI object path |

`check_context` giữ nguyên logic. Nếu `window == 0` (không có id cửa sổ) thì chỉ so `pid` + `app` + `generation` + `focus_signature`;
khi `focus_signature` cũng thiếu ⇒ `Unverifiable` (đúng như Windows hiện tại).

### 3.2. Luồng thay thế theo nền tảng

| Bước | Windows (hiện tại) | macOS | Linux |
|---|---|---|---|
| Bắt `mã + Space` | hook nuốt Space | tap trả `NULL` cho Space | Space đi vào ô nhập (không nuốt) |
| Xóa mã | `trigger_len` Backspace | `trigger_len` Backspace | `trigger_len + 1` Backspace |
| Chèn | clipboard + `Ctrl+V` | `NSPasteboard` + `Cmd+V` | clipboard + `Ctrl+V` (terminal: `Ctrl+Shift+V`, cấu hình theo app) |
| Từ chối (ngữ cảnh không an toàn) | phát lại Space đã nuốt | phát lại Space đã nuốt | không làm gì (Space đã có sẵn) |
| Đánh dấu sự kiện do app sinh ra | `dwExtraInfo = INJECT_MARKER` | `kCGEventSourceUserData = INJECT_MARKER` | evdev: bỏ qua thiết bị uinput của chính app (theo tên thiết bị); X11 XTest: cờ "đang chèn" + cửa sổ thời gian ngắn |

`Config.preserve_delimiter` trên Linux nghĩa là gõ lại Space sau khi chèn.

### 3.3. macOS

* **Quyền (TCC):** cần *Accessibility* (`AXIsProcessTrustedWithOptions` — tap chỉnh sửa sự kiện + đọc AX) và có thể cả *Input Monitoring*.
  Lần chạy đầu: màn hình hướng dẫn cấp quyền, nút mở System Settings; chưa có quyền ⇒ gõ tắt tắt, palette vẫn dùng được (chỉ sao chép).
* **Bắt phím:** `CGEventTapCreate(kCGSessionEventTap, kCGHeadInsertEventTap, …, keyDown|flagsChanged)` trên thread riêng có `CFRunLoop`.
  Ký tự lấy bằng `CGEventKeyboardGetUnicodeString` (theo layout hiện tại). Callback tuân thủ quy tắc hiện có: `try_lock`, `try_send`
  kênh 64, không I/O; panic ⇒ cho phím đi qua.
* **Tap bị vô hiệu:** khi nhận `kCGEventTapDisabledByTimeout` / `ByUserInput` ⇒ `CGEventTapEnable(tap, true)` và tăng bộ đếm (tương đương
  watchdog trên Windows, hiển thị ở Chẩn đoán).
* **Ô mật khẩu:** `IsSecureEventInputEnabled()` ⇒ không bắt/không chèn; thêm kiểm tra AX role `AXSecureTextField`.
* **Ứng dụng & ô nhập:** `NSWorkspace.frontmostApplication` (bundle id, pid) + thông báo `didActivateApplication` để tăng `generation`;
  `AXUIElementCopyAttributeValue(systemWide, kAXFocusedUIElementAttribute)` cho role/editable.
* **Chèn:** `CGEventCreateKeyboardEvent` (Backspace = keycode 51, `V` + `kCGEventFlagMaskCommand`), post tới `kCGHIDEventTap`;
  clipboard `NSPasteboard.general` với `changeCount` làm token (tương tự sequence number trên Windows); đặt kiểu
  `org.nspasteboard.TransientType` / `ConcealedType` để trình quản lý clipboard bỏ qua.
* **Phím tắt toàn cục:** `RegisterEventHotKey` (Carbon) — không cần quyền thêm; mặc định đổi `Ctrl` → `Cmd`/`Ctrl+Option` khi trùng
  phím hệ thống (ví dụ `Ctrl+Space` là chuyển bộ gõ).
* **Bộ gõ tiếng Việt:** bộ gõ Telex/VNI của macOS và OpenKey/EVKey dùng *marked text* (chữ đang soạn có gạch chân). Tap nhìn thấy phím
  thô trước bộ gõ; mã gõ tắt là ASCII nên khớp được, nhưng Backspace khi còn marked text có thể xóa sai. Phase 0 phải đo; phương án dự
  phòng: khi bộ gõ hiện tại (`TISCopyCurrentKeyboardInputSource`) là bộ gõ tiếng Việt thì chờ commit (bộ gõ commit khi gặp Space) trước khi
  gửi Backspace, hoặc tắt gõ tắt kèm thông báo.
* **Giao diện:** eframe chạy ở main thread; `tray-icon` (NSStatusItem) phải tạo trên main thread sau khi event loop khởi động.
  `Info.plist`: `LSUIElement = true` (ứng dụng menu bar, không hiện Dock), `NSAccessibilityUsageDescription`.
* **Hệ thống:** dữ liệu `~/Library/Application Support/PSnippet/`; single instance bằng `flock` trên file lock trong thư mục đó;
  autostart bằng `SMAppService.mainApp` (macOS 13+) hoặc LaunchAgent plist `~/Library/LaunchAgents/` (macOS 11–12).
  Font giao diện: `/System/Library/Fonts/SFNS.ttf`, dự phòng `Helvetica.ttc`.

### 3.4. Linux

* **Bắt phím (evdev):** liệt kê `/dev/input/event*` có `EV_KEY` + phím chữ (crate `evdev`), đọc bằng `epoll` trên thread riêng; theo dõi
  hot-plug bằng `inotify` trên `/dev/input`. Quyền: user thuộc nhóm `input` (gói `.deb` hướng dẫn / udev rule). Thiếu quyền ⇒ trang
  hướng dẫn, gõ tắt tắt.
* **Keycode → ký tự:** `xkbcommon` với keymap lấy từ X server (X11) hoặc từ cấu hình hệ thống (`localectl` / biến `XKB_DEFAULT_*`)
  trên Wayland. Nếu layout không xác định được ⇒ chỉ hỗ trợ mã ASCII trên layout US và ghi rõ ở Chẩn đoán.
* **Chèn:** X11 dùng XTest (`XTestFakeKeyEvent`); Wayland dùng thiết bị uinput "PSnippet virtual keyboard" (cần `/dev/uinput`
  ghi được — udev rule `KERNEL=="uinput", GROUP="input", MODE="0660"`). Clipboard qua `arboard` (X11 selection; Wayland data-control —
  GNOME không hỗ trợ data-control ⇒ fallback gõ từng ký tự Unicode qua uinput không khả thi với chữ có dấu ⇒ trên GNOME Wayland chỉ hỗ trợ
  **Copy-only** cho tới khi có phương án đã kiểm chứng).
* **Ứng dụng & ô nhập:** AT-SPI2 (crate `atspi`, D-Bus) — sự kiện `focus`/`state-changed:focused` cho ứng dụng và đối tượng đang focus,
  role `ROLE_PASSWORD_TEXT` ⇒ từ chối. Ứng dụng Electron/Chromium chỉ xuất cây accessibility khi bật accessibility (biến
  `ACCESSIBILITY_ENABLED=1` hoặc cờ `--force-renderer-accessibility`) — ghi vào hướng dẫn. X11 bổ sung `_NET_ACTIVE_WINDOW`,
  `_NET_WM_PID`, `WM_CLASS` để có window id và `generation` chính xác.
* **Allowlist:** mục là đường dẫn exe (`/proc/<pid>/exe`), `WM_CLASS` (X11) hoặc tên ứng dụng AT-SPI. Không xác định được ứng dụng ⇒
  không kích hoạt (fail-closed), trừ khi người dùng bật "Mọi ứng dụng".
* **Phím tắt toàn cục:** X11 `XGrabKey`; Wayland: nhận diện tổ hợp ngay trong luồng evdev (không nuốt được — chọn tổ hợp ít đụng độ,
  mặc định `Ctrl+Alt+P`), hoặc portal `org.freedesktop.portal.GlobalShortcuts` khi môi trường hỗ trợ.
* **Bộ gõ tiếng Việt:** IBus/Fcitx5 (ibus-bamboo, ibus-unikey, fcitx5-unikey) có preedit. evdev thấy phím thô; rủi ro giống macOS —
  Phase 0 đo trên ibus-bamboo và fcitx5-unikey, phương án dự phòng tương tự (chờ commit hoặc tắt kèm thông báo).
* **Giao diện:** eframe (winit, X11/Wayland). Tray qua `tray-icon` (libayatana-appindicator, cần GTK); GNOME cần extension AppIndicator —
  không có tray thì cửa sổ chính là điểm điều khiển duy nhất và đóng cửa sổ sẽ hỏi "Ẩn hay Thoát".
* **Hệ thống:** dữ liệu `$XDG_DATA_HOME/psnippet/` (mặc định `~/.local/share/psnippet/`); single instance bằng `flock` trên
  `$XDG_RUNTIME_DIR/psnippet.lock`; autostart bằng `~/.config/autostart/psnippet.desktop`; font qua fontconfig (`Noto Sans`,
  `DejaVu Sans`), dự phòng font egui.

### 3.5. Thay đổi phần dùng chung

* `crates/app`: bỏ mọi tham chiếu `pw::` trực tiếp; nhận `Box<dyn …>` từ `platform::current()`. Chuỗi phím tắt hiển thị theo nền tảng
  (`Cmd` trên macOS). Thông báo lỗi quyền (TCC, nhóm `input`, uinput) có trang hướng dẫn riêng.
* `crates/ui`: `install_fonts` nhận danh sách đường dẫn từ `System::ui_font_paths`; trang "Ứng dụng" hiển thị `AppId` theo nền tảng;
  trang Chẩn đoán thêm: backend (`win32` / `cgeventtap` / `evdev+x11` / `evdev+wayland`), trạng thái quyền, số lần tap/hook bị tắt.
* `.cargo/config.toml`: giữ `crt-static` chỉ cho MSVC (đã giới hạn bằng `cfg`). `build.rs` của `app`: chỉ nhúng icon khi target là Windows
  (đã có); macOS dùng `.icns` sinh từ `logo.rs`, Linux dùng PNG 256 px cho `.desktop` / AppImage.

### 3.6. Lộ trình

| Mốc | Nội dung | Điều kiện hoàn thành |
|---|---|---|
| P0 | Spike trên máy thật: macOS (Zalo Mac, Safari/Chrome, TextEdit; bộ gõ Telex hệ thống, OpenKey) và Linux (Ubuntu GNOME Wayland, KDE Wayland, X11; Chrome Zalo Web; ibus-bamboo, fcitx5-unikey) | Bảng tương thích có kết quả thật hoặc `untested`; quyết định chốt về bộ gõ tiếng Việt |
| P1 | Tách `zs-platform`, chuyển Windows sang trait | Toàn bộ test hiện có pass, hành vi Windows không đổi |
| P2 | `platform-macos` + đóng gói `.app` ký/notarize | Mục kiểm thử §5 macOS pass trên macOS 13–15 (Intel + Apple Silicon) |
| P3 | `platform-linux` (X11 trước, rồi Wayland) + AppImage/.deb | Mục kiểm thử §5 Linux pass trên Ubuntu 24.04 X11 + Wayland |
| P4 | CI ma trận (`windows-latest`, `macos-latest`, `ubuntu-latest`): build + unit test; release workflow sinh 3 loại gói | Release có đủ tệp cho 3 nền tảng |

### 3.7. Rủi ro

| Rủi ro | Mức | Giảm thiểu |
|---|---|---|
| Bộ gõ tiếng Việt (marked text / preedit) làm Backspace xóa sai | Cao | Phase 0; chờ commit; tắt kèm thông báo khi phát hiện bộ gõ chưa hỗ trợ |
| Wayland không cho biết ứng dụng đang focus | Cao | AT-SPI; fail-closed; tùy chọn "Mọi ứng dụng" có cảnh báo |
| GNOME Wayland không có clipboard data-control | Cao | Copy-only trên GNOME Wayland cho tới khi có giải pháp kiểm chứng |
| Quyền macOS (TCC) gây bối rối / bị thu hồi khi cập nhật bản không ký | Trung bình | Ký Developer ID ổn định, màn hình hướng dẫn, kiểm tra quyền mỗi lần khởi động |
| evdev cần nhóm `input` — quyền đọc mọi bàn phím | Trung bình | Chỉ đọc, không log, không lưu phím; nêu rõ trong README/SECURITY |
| Tray không hiện trên GNOME | Thấp | Cửa sổ chính thay thế; hướng dẫn cài AppIndicator |

## 4. API / Schema changes

* **Mới:** crate `zs-platform` (trait + `WindowId`, `AppId`, `PasteChord`, `Token`); crate `platform-macos`, `platform-linux`.
* **Breaking (nội bộ workspace):** `TargetContext.hwnd: isize` → `window: WindowId`; `executable_path: String` → `app: AppId`.
  `HookHandle`/`HookEvent` chuyển sang `zs-platform`. Không ảnh hưởng người dùng.
* **Config (không breaking, serde default):** `allowed_apps` giữ `Vec<String>` nhưng mỗi mục có tiền tố nền tảng khi lưu mới
  (`win:` đường dẫn, `mac:` bundle id, `linux:exe:` / `linux:class:` / `linux:atspi:`); mục không tiền tố được hiểu là Windows (tương thích dữ liệu cũ).
  Thêm `allow_all_apps: bool` (mặc định `false`), `paste_chord_overrides: Map<AppId, PasteChord>`.
* **SQLite / `.zsnip`:** không đổi. Thư mục dữ liệu theo chuẩn từng nền tảng (§3.3, §3.4); file `.zsnip` và bản sao lưu dùng chéo được giữa các nền tảng.

## 5. How to test

* **Tự động (CI, mọi nền tảng):** `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace --locked`; unit test cho
  ánh xạ keycode→ký tự (bảng xkb mẫu), chuyển đổi `AppId`/tiền tố allowlist, `check_context` khi `window == 0`, sinh `.icns`/PNG từ logo.
* **Thủ công macOS:** chưa cấp quyền ⇒ gõ tắt tắt + hướng dẫn; cấp Accessibility ⇒ TextEdit `zok`+Space được thay, Space không bị nhân đôi;
  ô mật khẩu (Safari) không kích hoạt; `Cmd+Tab` giữa lúc chèn ⇒ dừng, chỉ sao chép; bộ gõ Telex hệ thống + OpenKey; clipboard được khôi phục;
  tap bị tắt (giả lập treo) ⇒ tự bật lại; tự khởi động sau đăng nhập.
* **Thủ công Linux (X11 và Wayland):** không thuộc nhóm `input` ⇒ hướng dẫn; gedit/Chrome (Zalo Web) `zok`+Space ⇒ mã và Space bị xóa,
  nội dung được chèn; terminal dùng `Ctrl+Shift+V`; ô mật khẩu không kích hoạt; ứng dụng không trong allowlist / không xác định ⇒ không kích hoạt;
  GNOME Wayland ⇒ Copy-only kèm thông báo; ibus-bamboo và fcitx5-unikey; rút/cắm bàn phím USB khi đang chạy.
* **Chung:** không có thao tác Enter/Send ở bất kỳ luồng nào (kiểm tra bằng review trait `Injector` và test FSM hiện có); log không chứa nội dung gõ.
