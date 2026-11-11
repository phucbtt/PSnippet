# 2026-11-11 — Form không chèn được vào ô tin nhắn (strict mode)

## 1. Context / Why
Sau khi điền form và bấm Chèn, nội dung không vào ô chat mà chỉ nằm trong clipboard. Nguyên nhân: strict mode (mặc định) +
`chat_signature` luôn `None` ⇒ `check_context` = `Unverifiable` ⇒ `revalidate_for_confirm` trả `TargetInvalid` ⇒ FSM sang
`CopyOnlySafe` (chỉ copy). Nút Chèn vẫn sáng nên gây hiểu nhầm; lý do không được ghi log.

## 2. Decision
Giữ fail-closed nhưng cho người dùng xác nhận tường minh: form hiện ô tick "đã kiểm tra đúng cuộc trò chuyện"; chưa tick thì Chèn bị khóa
(Sao chép vẫn dùng được). Đã tick ⇒ `Unverifiable` được chấp nhận; hwnd/pid/thời điểm tạo process/generation/focus vẫn phải khớp (`Changed` vẫn bị chặn).
Phương án loại: đổi mặc định `strict_focus=false` (mất cảnh báo), hoặc bỏ guard (vi phạm spec §6A.7).

## 3. Design
`ui/form.rs`: `needs_chat_ack`, `chat_ack`, `FormOutcome::Insert(segs, ack)`. `app/orchestrator.rs`: `Pending.chat_ack`; `begin_compose` bật
`needs_chat_ack` khi strict; `revalidate_for_confirm` chấp nhận `Unverifiable` khi `chat_ack`; log lý do từ chối (trang Diagnostics).

## 4. API / Schema changes
`FormOutcome::Insert` đổi tuple (nội bộ, không public ngoài workspace). Không đổi schema/config.

## 5. How to test
Zalo (hoặc Notepad đã thêm vào App Rules): gõ mã có `[biến]` + Space → điền → thấy ô tick, Chèn bị mờ → tick → Chèn: trigger bị xóa, bản nháp vào ô chat.
Chuyển sang cửa sổ khác trước khi Chèn ⇒ bị chặn, log `revalidate: Changed`. **Chưa thử trên Zalo/Notepad thật.** `cargo test --workspace` pass.
