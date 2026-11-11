//! Foreground/process identity and TargetContext snapshots.

use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};

use windows::Win32::Foundation::{CloseHandle, FILETIME, HWND, MAX_PATH};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GUITHREADINFO, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, IsIconic,
    IsWindow, SW_RESTORE, SetForegroundWindow, ShowWindow,
};
use windows::core::PWSTR;
use zs_mention::TargetContext;

/// Bumped by the hook thread on every foreground change (and by anyone who observes a focus change).
pub static GENERATION: AtomicU64 = AtomicU64::new(1);

pub fn bump_generation() -> u64 {
    GENERATION.fetch_add(1, Ordering::SeqCst) + 1
}

pub fn generation() -> u64 {
    GENERATION.load(Ordering::SeqCst)
}

/// Our own top-level window: foreground changes to/from it must not look like "the user switched chat".
static IGNORE_HWND: AtomicIsize = AtomicIsize::new(0);
/// One-shot: the next foreground change to this hwnd was requested by us (restoring the target).
static EXPECT_HWND: AtomicIsize = AtomicIsize::new(0);

pub fn set_ignore_hwnd(h: isize) {
    IGNORE_HWND.store(h, Ordering::SeqCst);
}

/// Called by the foreground WinEvent: true ⇒ do not bump the generation.
pub(crate) fn is_expected_or_ignored(h: isize) -> bool {
    h == IGNORE_HWND.load(Ordering::SeqCst)
        || EXPECT_HWND
            .compare_exchange(h, 0, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
}

/// Bring `hwnd` back to the foreground (used after our popup closes) and wait briefly for it.
/// Windows may refuse (foreground lock); the caller must treat `false` as "do not inject".
pub fn focus_window(hwnd: isize) -> bool {
    let h = HWND(hwnd as *mut _);
    if !window_exists(hwnd) {
        return false;
    }
    if foreground_hwnd().is_some_and(|f| f.0 as isize == hwnd) {
        return true;
    }
    EXPECT_HWND.store(hwnd, Ordering::SeqCst);
    unsafe {
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        let _ = SetForegroundWindow(h);
    }
    for _ in 0..25 {
        if foreground_hwnd().is_some_and(|f| f.0 as isize == hwnd) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    EXPECT_HWND.store(0, Ordering::SeqCst);
    false
}

pub fn foreground_hwnd() -> Option<HWND> {
    let h = unsafe { GetForegroundWindow() };
    (!h.0.is_null()).then_some(h)
}

pub fn window_exists(hwnd: isize) -> bool {
    unsafe { IsWindow(Some(HWND(hwnd as *mut _))).as_bool() }
}

pub fn window_pid(hwnd: HWND) -> Option<u32> {
    let mut pid = 0u32;
    let tid = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    (tid != 0 && pid != 0).then_some(pid)
}

/// Full image path of a process (needs only PROCESS_QUERY_LIMITED_INFORMATION; works for same-or-lower
/// integrity processes, returns None for protected ones — never tries to bypass).
pub fn process_image_path(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; (MAX_PATH as usize) * 4];
        let mut len = buf.len() as u32;
        let r =
            QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        r.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

pub fn process_start_time(pid: u32) -> Option<u64> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let (mut c, mut e, mut k, mut u) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        let r = GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u);
        let _ = CloseHandle(h);
        r.ok()?;
        Some(((c.dwHighDateTime as u64) << 32) | c.dwLowDateTime as u64)
    }
}

pub fn foreground_exe() -> Option<String> {
    window_pid(foreground_hwnd()?).and_then(process_image_path)
}

/// Does `exe_path` match an allowlist entry (full path or bare file name, case-insensitive)?
pub fn exe_matches(exe_path: &str, entry: &str) -> bool {
    let a = exe_path.to_lowercase();
    let b = entry.trim().to_lowercase();
    if b.is_empty() {
        return false;
    }
    if a == b {
        return true;
    }
    let file = a.rsplit(['\\', '/']).next().unwrap_or(&a);
    !b.contains(['\\', '/']) && file == b
}

/// HWND of the focused control inside the foreground window's GUI thread (may be null for
/// frameworks that draw their own caret, e.g. Chromium/Electron).
pub fn focus_hwnd(fg: HWND) -> Option<isize> {
    unsafe {
        let tid = GetWindowThreadProcessId(fg, None);
        let mut info = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        GetGUIThreadInfo(tid, &mut info).ok()?;
        (!info.hwndFocus.0.is_null()).then_some(info.hwndFocus.0 as isize)
    }
}

/// Snapshot of the current foreground target. `chat_signature` stays `None` until a Zalo-specific
/// UIA probe is validated on the real client (Phase 0 spike) — so in strict mode a Zalo target is
/// "Unverifiable" and falls back to Copy-only, exactly as the spec's fail-closed policy demands.
pub fn snapshot_foreground(
    probe_timeout: std::time::Duration,
) -> Option<(TargetContext, Option<crate::uia::FocusProbe>)> {
    let fg = foreground_hwnd()?;
    let pid = window_pid(fg)?;
    let focus = crate::uia::probe_focus(probe_timeout);
    let ctx = TargetContext {
        hwnd: fg.0 as isize,
        pid,
        process_start_time: process_start_time(pid)?,
        executable_path: process_image_path(pid)?,
        focus_signature: focus.as_ref().and_then(|f| f.runtime_id.clone()),
        chat_signature: None,
        generation: generation(),
    };
    Some((ctx, focus))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_matching() {
        assert!(exe_matches(
            r"C:\Users\x\AppData\Local\Zalo\Zalo.exe",
            "zalo.exe"
        ));
        assert!(exe_matches(r"C:\A\B.exe", r"c:\a\b.exe"));
        assert!(!exe_matches(r"C:\A\notzalo.exe", "zalo.exe"));
        assert!(!exe_matches(r"C:\A\B.exe", r"C:\Other\B.exe"));
        assert!(!exe_matches(r"C:\A\B.exe", ""));
    }
}
