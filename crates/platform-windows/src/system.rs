//! Single instance + opt-in autostart (HKCU Run).

use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW, RegOpenKeyExW,
    RegSetValueExW,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::{HSTRING, PCWSTR, w};

use crate::{PlatformError, Result};

/// Returns the mutex handle (keep alive for process lifetime) or `None` if another instance runs.
pub fn acquire_single_instance() -> Option<HANDLE> {
    unsafe {
        let h = CreateMutexW(None, true, w!("Local\\PSnippet.SingleInstance")).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return None;
        }
        Some(h)
    }
}

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE: PCWSTR = w!("PSnippet");
/// Pre-rename autostart value; removed so the old exe path does not linger.
const OLD_VALUE: PCWSTR = w!("ZaloSnippet");

/// Enable/disable "Start with Windows" for the current user only (no admin needed).
pub fn set_autostart(enabled: bool) -> Result<()> {
    unsafe {
        let mut key = HKEY::default();
        RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_SET_VALUE, &mut key)
            .ok()
            .map_err(PlatformError::from)?;
        let _ = RegDeleteValueW(key, OLD_VALUE);
        let res = if enabled {
            let exe = std::env::current_exe().map_err(|e| PlatformError::Other(e.to_string()))?;
            let cmd = HSTRING::from(format!("\"{}\" --background", exe.display()));
            let bytes: &[u8] =
                std::slice::from_raw_parts(cmd.as_ptr() as *const u8, (cmd.len() + 1) * 2);
            RegSetValueExW(key, VALUE, None, REG_SZ, Some(bytes)).ok()
        } else {
            let r = RegDeleteValueW(key, VALUE);
            // Deleting a value that does not exist is fine.
            if r.0 == 2 { Ok(()) } else { r.ok() }
        };
        let _ = RegCloseKey(key);
        res.map_err(PlatformError::from)
    }
}
