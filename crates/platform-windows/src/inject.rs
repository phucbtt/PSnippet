//! SendInput helpers (spec §6.2). Every event carries INJECT_MARKER in dwExtraInfo.
//! There is intentionally NO function that sends Enter/Return — the app never submits a chat message.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, SendInput, VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_V,
};

use crate::{INJECT_MARKER, PlatformError, Result};

fn key(vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: INJECT_MARKER,
            },
        },
    }
}

fn unicode_unit(unit: u16, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unit,
                dwFlags: KEYEVENTF_UNICODE
                    | if up {
                        KEYEVENTF_KEYUP
                    } else {
                        KEYBD_EVENT_FLAGS(0)
                    },
                time: 0,
                dwExtraInfo: INJECT_MARKER,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> Result<()> {
    if inputs.is_empty() {
        return Ok(());
    }
    let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        // Blocked (UIPI), or another thread holds the input lock.
        return Err(PlatformError::Other(
            "E_UIPI_BLOCKED: SendInput rejected".into(),
        ));
    }
    Ok(())
}

/// Press Backspace `n` times — exactly the number of trigger characters the user typed.
pub fn backspaces(n: usize) -> Result<()> {
    let mut v = Vec::with_capacity(n * 2);
    for _ in 0..n {
        v.push(key(VK_BACK, KEYBD_EVENT_FLAGS(0)));
        v.push(key(VK_BACK, KEYEVENTF_KEYUP));
    }
    send(&v)
}

/// Ctrl+V.
pub fn ctrl_v() -> Result<()> {
    send(&[
        key(VK_CONTROL, KEYBD_EVENT_FLAGS(0)),
        key(VK_V, KEYBD_EVENT_FLAGS(0)),
        key(VK_V, KEYEVENTF_KEYUP),
        key(VK_CONTROL, KEYEVENTF_KEYUP),
    ])
}

/// Fallback: type text as Unicode key events (surrogate pairs sent as two units).
/// Refuses text containing line breaks: Enter in a chat box may *send* the message.
pub fn type_unicode_single_line(text: &str) -> Result<()> {
    if text.contains(['\n', '\r']) {
        return Err(PlatformError::Other(
            "multiline text cannot be typed safely; use clipboard paste".into(),
        ));
    }
    let mut v = Vec::new();
    for unit in text.encode_utf16() {
        v.push(unicode_unit(unit, false));
        v.push(unicode_unit(unit, true));
    }
    send(&v)
}

/// Clipboard paste with a single best-effort restore of the user's original clipboard at the end.
///
/// The snapshot is taken once at `begin`; every `paste` writes our text (excluded from clipboard
/// history) and sends Ctrl+V; `finish` restores the original only if the clipboard still holds *our*
/// last write (sequence number unchanged), after a short delay so the target can read it first.
/// The delay is a restore-safety margin, not proof that the paste rendered (§6.2 #4).
pub struct PasteSession {
    snapshot: Option<crate::clipboard::Snapshot>,
    last_seq: Option<u32>,
}

impl PasteSession {
    pub fn begin(restore: bool) -> Self {
        Self {
            snapshot: if restore {
                crate::clipboard::snapshot().ok()
            } else {
                None
            },
            last_seq: None,
        }
    }

    /// Write `text` and press Ctrl+V. On `Err` the text may already be on the clipboard so the user
    /// can paste it manually.
    pub fn paste(&mut self, text: &str) -> Result<()> {
        self.last_seq = Some(crate::clipboard::set_text(text)?);
        ctrl_v()
    }

    pub fn finish(mut self) {
        if let (Some(snap), Some(seq)) = (self.snapshot.take(), self.last_seq) {
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(700));
                let _ = crate::clipboard::restore_if_unchanged(&snap, seq);
            });
        }
    }
}

/// Re-emit a Tab the hook swallowed when a trigger turned out not to be usable.
pub fn tab() -> Result<()> {
    send(&[
        key(
            windows::Win32::UI::Input::KeyboardAndMouse::VK_TAB,
            KEYBD_EVENT_FLAGS(0),
        ),
        key(
            windows::Win32::UI::Input::KeyboardAndMouse::VK_TAB,
            KEYEVENTF_KEYUP,
        ),
    ])
}

/// Re-emit a Space the hook swallowed when a trigger turned out not to be usable.
pub fn space() -> Result<()> {
    type_unicode_single_line(" ")
}

/// Tap Alt (marked as ours) so Windows lets this process take the foreground, then activate `hwnd`.
/// Only used for our own windows.
pub fn activate_own_window(hwnd: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_MENU;
    use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
    let _ = send(&[
        key(VK_MENU, KEYBD_EVENT_FLAGS(0)),
        key(VK_MENU, KEYEVENTF_KEYUP),
    ]);
    unsafe {
        let _ = SetForegroundWindow(HWND(hwnd as *mut _));
    }
}
