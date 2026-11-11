//! Windows adapter (spec §6.1/§6.2). ALL `unsafe` / FFI lives in this crate.
//!
//! * `hook`      – WH_KEYBOARD_LL / WH_MOUSE_LL / WinEvent / RegisterHotKey on one dedicated thread
//! * `context`   – foreground window, process identity, TargetContext snapshots
//! * `uia`       – UI Automation worker (COM MTA) for focus/password probing
//! * `clipboard` – CF_UNICODETEXT write, best-effort snapshot/restore, history exclusion
//! * `inject`    – SendInput (Backspace, Ctrl+V, Unicode typing). There is NO Enter/Send helper.
//! * `system`    – single instance, autostart
#![cfg(windows)]

pub mod clipboard;
pub mod context;
pub mod hook;
pub mod inject;
pub mod system;
pub mod uia;

/// Marker placed in `dwExtraInfo` of every event we synthesize ("ZSNP").
pub const INJECT_MARKER: usize = 0x5A53_4E50;

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("win32: {0}")]
    Win32(String),
    #[error("E_CLIPBOARD_BUSY")]
    ClipboardBusy,
    #[error("E_FOCUS_UNKNOWN: {0}")]
    FocusUnknown(String),
    #[error("{0}")]
    Other(String),
}

impl From<windows::core::Error> for PlatformError {
    fn from(e: windows::core::Error) -> Self {
        PlatformError::Win32(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, PlatformError>;
