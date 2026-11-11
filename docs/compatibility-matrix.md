# Compatibility matrix

Không có mục nào dưới đây được nâng lên `tested` nếu chưa có bằng chứng từ máy thật (spec §12, §20.2).

```yaml
platform: Windows 11 x64
zalo_build: "TO_BE_RECORDED_ON_TEST_DEVICE"
app_executable: "DETECTED_BY_RUNTIME (App Rules → 'Thêm ứng dụng đang dùng')"
profile: "zalo-pc-manual-v1"
keyboard_ime: "UniKey-Telex"
text_paste: untested
multiline: untested
mention_manual: untested
mention_assisted: not implemented
mention_auto_verified: disabled
observed_uia_tree: unknown
qa_operator: ""
qa_date: ""
notes: "Do not promote to tested without physical Zalo PC evidence"
```

| Khu vực | Trạng thái |
|---|---|
| Parser, template, matcher, FSM, storage | Unit/contract test pass (không phụ thuộc Zalo) |
| Hook / tray / egui / paste trên máy thật | Biên dịch + clippy sạch; **chưa quan sát chạy** |
| Notepad | Có script thủ công `tests/manual/smoke-notepad.ps1`, chưa chạy |
| Zalo PC (IT-01..04, 08; MN-01..) | untested — cần Phase 0 spike |

Hệ quả thiết kế đã áp dụng cho tới khi có dữ liệu: `chat_signature = None` ⇒ strict mode ⇒ Copy-only cho form; chế độ tương thích
(Settings → bỏ "Strict") là lựa chọn có cảnh báo; M1/M2 bị hạ về M0.
