//! Keyboard/mouse hooks, foreground tracking and global hotkeys on ONE dedicated thread with a
//! message loop (spec §6, LowLevelKeyboardProc rules).
//!
//! Hook callbacks only: read a few atomics, feed the in-memory `TriggerMatcher` (try_lock — never
//! waits), `try_send` an event on a bounded channel. No DB, no UIA, no clipboard, no disk, no waits.
//! If anything is uncertain (lock busy, queue full, panic) the key is passed through untouched.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use zs_core::{AppScope, Delim, KeyInput, Match, MatcherConfig, TriggerMatcher};

use crate::context::{bump_generation, foreground_hwnd, process_image_path, window_pid};
use crate::{INJECT_MARKER, PlatformError, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyAction {
    Palette,
    Resume,
    CancelSession,
    /// Toggle hotstring expansion on/off (same as tray Pause/Resume).
    TogglePause,
}

impl HotkeyAction {
    fn id(self) -> i32 {
        match self {
            Self::Palette => 1,
            Self::Resume => 2,
            Self::CancelSession => 3,
            Self::TogglePause => 4,
        }
    }
    fn from_id(id: i32) -> Option<Self> {
        match id {
            1 => Some(Self::Palette),
            2 => Some(Self::Resume),
            3 => Some(Self::CancelSession),
            4 => Some(Self::TogglePause),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum HookEvent {
    /// A trigger matched; the delimiter (if any) was swallowed by the hook.
    Trigger {
        m: Match,
        hwnd: isize,
    },
    Hotkey(HotkeyAction),
    /// `RegisterHotKey` failed (conflict) — UI must let the user change the combination.
    HotkeyUnavailable(HotkeyAction),
    /// The event queue was full; the key was passed through (fail open).
    Overflow,
}

pub struct HookConfig {
    pub matcher: MatcherConfig,
    pub codes: Vec<String>,
    /// Which apps hotstrings may fire in (all-except-blocked, or an allowlist).
    pub scope: AppScope,
    /// "Ctrl+Alt+Space"-style strings.
    pub hotkeys: Vec<(HotkeyAction, String)>,
}

pub struct Shared {
    matcher: Mutex<TriggerMatcher>,
    paused: AtomicBool,
    fg_allowed: AtomicBool,
    scope: RwLock<AppScope>,
    overflow: AtomicU64,
    /// Tick count (ms) of the last event seen by either low-level hook; used by the watchdog.
    last_hook_tick: AtomicU32,
    /// How many times the watchdog had to reinstall the hooks (diagnostics).
    reinstalls: AtomicU64,
    /// Keys swallowed by an IME (VK_PROCESSKEY) since startup (diagnostics).
    ime_keys: AtomicU64,
    tx: SyncSender<HookEvent>,
}

impl Shared {
    pub fn set_paused(&self, p: bool) {
        self.paused.store(p, Ordering::SeqCst);
        if let Ok(mut m) = self.matcher.lock() {
            m.reset();
        }
    }
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }
    pub fn set_codes<'a>(&self, codes: impl IntoIterator<Item = &'a str>) {
        if let Ok(mut m) = self.matcher.lock() {
            m.set_codes(codes);
        }
    }
    pub fn set_matcher_config(&self, cfg: MatcherConfig) {
        if let Ok(mut m) = self.matcher.lock() {
            m.set_config(cfg);
        }
    }
    pub fn set_app_scope(&self, scope: AppScope) {
        if let Ok(mut a) = self.scope.write() {
            *a = scope;
        }
        self.refresh_foreground();
    }
    /// Reset typed-text state (e.g. when our own popup opens/closes).
    pub fn reset_buffer(&self) {
        if let Ok(mut m) = self.matcher.lock() {
            m.reset();
        }
    }
    pub fn reinstall_count(&self) -> u64 {
        self.reinstalls.load(Ordering::Relaxed)
    }
    pub fn ime_key_count(&self) -> u64 {
        self.ime_keys.load(Ordering::Relaxed)
    }
    fn stamp(&self) {
        self.last_hook_tick
            .store(unsafe { GetTickCount() }, Ordering::Relaxed);
    }
    pub fn overflow_count(&self) -> u64 {
        self.overflow.load(Ordering::Relaxed)
    }
    fn refresh_foreground(&self) {
        let allowed = foreground_hwnd().is_some_and(|h| self.is_allowed_window(h));
        self.fg_allowed.store(allowed, Ordering::SeqCst);
        self.reset_buffer();
    }
    fn is_allowed_window(&self, hwnd: HWND) -> bool {
        let Some(pid) = window_pid(hwnd) else {
            return false;
        };
        if pid == std::process::id() {
            return false; // never expand inside PSnippet's own window (editor, form, palette)
        }
        let Some(path) = process_image_path(pid) else {
            return false; // cannot identify (e.g. elevated app) ⇒ fail closed
        };
        self.scope.read().map(|s| s.permits(&path)).unwrap_or(false)
    }
}

static CURRENT: RwLock<Option<Arc<Shared>>> = RwLock::new(None);

fn current() -> Option<Arc<Shared>> {
    CURRENT.try_read().ok().and_then(|g| g.clone())
}

fn key_down(vk: VIRTUAL_KEY) -> bool {
    (unsafe { GetAsyncKeyState(vk.0 as i32) } as u16 & 0x8000) != 0
}

fn is_modifier_vk(vk: u32) -> bool {
    [
        VK_SHIFT,
        VK_LSHIFT,
        VK_RSHIFT,
        VK_CONTROL,
        VK_LCONTROL,
        VK_RCONTROL,
        VK_MENU,
        VK_LMENU,
        VK_RMENU,
        VK_LWIN,
        VK_RWIN,
        VK_CAPITAL,
        VK_NUMLOCK,
        VK_SCROLL,
    ]
    .iter()
    .any(|k| k.0 as u32 == vk)
}

fn is_navigation_vk(vk: u32) -> bool {
    [
        VK_LEFT, VK_RIGHT, VK_UP, VK_DOWN, VK_HOME, VK_END, VK_PRIOR, VK_NEXT, VK_DELETE,
        VK_ESCAPE, VK_INSERT,
    ]
    .iter()
    .any(|k| k.0 as u32 == vk)
}

/// Translate a key-down into zero or more characters using the foreground thread's layout.
/// Flag 4 = do not modify keyboard state (so dead keys / IME state are not disturbed).
fn translate_chars(kb: &KBDLLHOOKSTRUCT, shift: bool, altgr: bool) -> Vec<char> {
    unsafe {
        let mut state = [0u8; 256];
        if shift {
            state[VK_SHIFT.0 as usize] = 0x80;
        }
        if altgr {
            state[VK_CONTROL.0 as usize] = 0x80;
            state[VK_MENU.0 as usize] = 0x80;
        }
        if (GetKeyState(VK_CAPITAL.0 as i32) & 1) != 0 {
            state[VK_CAPITAL.0 as usize] = 1;
        }
        let tid = foreground_hwnd()
            .map(|h| GetWindowThreadProcessId(h, None))
            .unwrap_or(0);
        let hkl = GetKeyboardLayout(tid);
        let mut buf = [0u16; 8];
        let n = ToUnicodeEx(kb.vkCode, kb.scanCode, &state, &mut buf, 4, Some(hkl));
        if n <= 0 {
            return vec![]; // dead key or no character
        }
        String::from_utf16_lossy(&buf[..n as usize])
            .chars()
            .filter(|c| !c.is_control())
            .collect()
    }
}

fn handle_key(kb: &KBDLLHOOKSTRUCT) -> bool {
    let Some(sh) = current() else { return false };
    if sh.paused.load(Ordering::Relaxed) || !sh.fg_allowed.load(Ordering::Relaxed) {
        return false;
    }
    let vk = kb.vkCode;
    if is_modifier_vk(vk) {
        return false;
    }
    if vk == VK_PROCESSKEY.0 as u32 {
        // An IME is composing: the characters are not final, so the typed-text window is unreliable
        // (spec §3.1). Drop it instead of guessing; never swallow the key.
        sh.ime_keys.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut m) = sh.matcher.try_lock() {
            m.reset();
        }
        return false;
    }
    let Ok(mut matcher) = sh.matcher.try_lock() else {
        return false;
    };

    let ctrl = key_down(VK_CONTROL);
    let alt = key_down(VK_MENU);
    let win = key_down(VK_LWIN) || key_down(VK_RWIN);
    let altgr = ctrl && alt; // AltGr is reported as Ctrl+Alt
    if win || (ctrl ^ alt) {
        matcher.reset(); // shortcut chord, not typing
        return false;
    }
    if is_navigation_vk(vk) {
        matcher.reset();
        return false;
    }

    let mut result: Option<Match> = None;
    if vk == VK_BACK.0 as u32 {
        matcher.feed(KeyInput::Backspace);
    } else if vk == VK_SPACE.0 as u32 {
        result = matcher.feed(KeyInput::Delimiter(Delim::Space));
    } else if vk == VK_TAB.0 as u32 {
        result = matcher.feed(KeyInput::Delimiter(Delim::Tab));
    } else if vk == VK_RETURN.0 as u32 {
        matcher.feed(KeyInput::Delimiter(Delim::Enter)); // never a trigger; resets the window
    } else {
        for c in translate_chars(kb, key_down(VK_SHIFT), altgr) {
            if let Some(m) = matcher.feed(KeyInput::Char(c)) {
                result = Some(m);
            }
        }
    }
    drop(matcher);

    let Some(m) = result else { return false };
    let swallow = m.delimiter.is_some();
    let hwnd = foreground_hwnd().map(|h| h.0 as isize).unwrap_or(0);
    match sh.tx.try_send(HookEvent::Trigger { m, hwnd }) {
        Ok(()) => swallow,
        Err(_) => {
            sh.overflow.fetch_add(1, Ordering::Relaxed);
            let _ = sh.tx.try_send(HookEvent::Overflow);
            false // queue full ⇒ let the key through, never eat input we cannot handle
        }
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        if let Some(sh) = current() {
            sh.stamp();
        }
        let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let msg = wparam.0 as u32;
        // Skip our own synthesized events only (by marker). IME-generated injected keys are processed
        // normally so Telex/VNI output keeps the window consistent (spec §3.1).
        if (msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN)
            && kb.dwExtraInfo != INJECT_MARKER
            && std::panic::catch_unwind(|| handle_key(kb)).unwrap_or(false)
        {
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        if let Some(sh) = current() {
            sh.stamp();
        }
        let msg = wparam.0 as u32;
        if matches!(msg, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN)
            && let Some(sh) = current()
            && let Ok(mut m) = sh.matcher.try_lock()
        {
            m.reset(); // caret may have moved
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn foreground_proc(
    _h: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _o: i32,
    _c: i32,
    _t: u32,
    _ms: u32,
) {
    if !crate::context::is_expected_or_ignored(hwnd.0 as isize) {
        bump_generation();
    }
    if let Some(sh) = current() {
        sh.refresh_foreground();
    }
}

/// Parse "Ctrl+Alt+Space" / "Ctrl+Alt+Shift+M" into (modifiers, virtual-key).
pub fn parse_hotkey(s: &str) -> Option<(u32, u32)> {
    let mut mods = 0u32;
    let mut vk = None;
    for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods |= MOD_CONTROL.0,
            "alt" => mods |= MOD_ALT.0,
            "shift" => mods |= MOD_SHIFT.0,
            "win" | "super" => mods |= MOD_WIN.0,
            "space" => vk = Some(VK_SPACE.0 as u32),
            "enter" => vk = Some(VK_RETURN.0 as u32),
            "tab" => vk = Some(VK_TAB.0 as u32),
            k if k.len() == 1 && k.as_bytes()[0].is_ascii_alphanumeric() => {
                vk = Some(k.to_ascii_uppercase().as_bytes()[0] as u32)
            }
            k if k.starts_with('f')
                && k[1..].parse::<u32>().is_ok_and(|n| (1..=24).contains(&n)) =>
            {
                vk = Some(VK_F1.0 as u32 + k[1..].parse::<u32>().ok()? - 1)
            }
            _ => return None,
        }
    }
    // A global hotkey needs at least one modifier, otherwise it would hijack a normal key.
    (mods != 0).then_some(())?;
    vk.map(|v| (mods, v))
}

pub struct HookHandle {
    pub shared: Arc<Shared>,
    thread_id: u32,
    join: Option<JoinHandle<()>>,
}

impl HookHandle {
    /// Install hooks + hotkeys. Returns the handle and the bounded event receiver.
    pub fn start(cfg: HookConfig) -> Result<(HookHandle, Receiver<HookEvent>)> {
        let (tx, rx) = sync_channel::<HookEvent>(64);
        let mut matcher = TriggerMatcher::new(cfg.matcher.clone());
        matcher.set_codes(cfg.codes.iter().map(String::as_str));
        let shared = Arc::new(Shared {
            matcher: Mutex::new(matcher),
            paused: AtomicBool::new(false),
            fg_allowed: AtomicBool::new(false),
            scope: RwLock::new(cfg.scope.clone()),
            overflow: AtomicU64::new(0),
            last_hook_tick: AtomicU32::new(unsafe { GetTickCount() }),
            reinstalls: AtomicU64::new(0),
            ime_keys: AtomicU64::new(0),
            tx: tx.clone(),
        });
        if let Ok(mut c) = CURRENT.write() {
            *c = Some(shared.clone());
        }
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<std::result::Result<u32, String>>();
        let hotkeys = cfg.hotkeys.clone();
        let sh2 = shared.clone();
        let join = std::thread::Builder::new()
            .name("zs-hook".into())
            .spawn(move || run_hook_thread(sh2, hotkeys, tx, ready_tx))
            .map_err(|e| PlatformError::Other(e.to_string()))?;
        let thread_id = ready_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| PlatformError::Other("hook thread did not start".into()))?
            .map_err(PlatformError::Other)?;
        shared.refresh_foreground();
        Ok((
            HookHandle {
                shared,
                thread_id,
                join: Some(join),
            },
            rx,
        ))
    }

    pub fn stop(&mut self) {
        if let Some(j) = self.join.take() {
            unsafe {
                let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
            let _ = j.join();
            if let Ok(mut c) = CURRENT.write() {
                *c = None;
            }
        }
    }
}

impl Drop for HookHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

const WATCHDOG_PERIOD_MS: u32 = 2000;
/// Input newer than the last hook event by more than this ⇒ hooks are considered dead.
const HOOK_STALE_MS: u32 = 3000;

fn install_keyboard(hmod: windows::Win32::Foundation::HMODULE) -> windows::core::Result<HHOOK> {
    unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), Some(hmod.into()), 0) }
}

fn install_mouse(hmod: windows::Win32::Foundation::HMODULE) -> Option<HHOOK> {
    unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), Some(hmod.into()), 0).ok() }
}

/// Pure decision used by the watchdog: `last_input` and `last_hook` are `GetTickCount` values.
pub(crate) fn is_stale(last_input: u32, last_hook: u32) -> bool {
    // Wrapping arithmetic; "input is newer than the hook stamp by more than the threshold".
    let delta = last_input.wrapping_sub(last_hook);
    delta != 0 && delta < u32::MAX / 2 && delta > HOOK_STALE_MS
}

fn hooks_look_dead(shared: &Shared) -> bool {
    unsafe {
        let mut li = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if !GetLastInputInfo(&mut li).as_bool() {
            return false;
        }
        is_stale(li.dwTime, shared.last_hook_tick.load(Ordering::Relaxed))
    }
}

fn run_hook_thread(
    shared: Arc<Shared>,
    hotkeys: Vec<(HotkeyAction, String)>,
    tx: SyncSender<HookEvent>,
    ready: std::sync::mpsc::Sender<std::result::Result<u32, String>>,
) {
    unsafe {
        let hmod = match GetModuleHandleW(None) {
            Ok(h) => h,
            Err(e) => {
                let _ = ready.send(Err(e.to_string()));
                return;
            }
        };
        let mut kb = match install_keyboard(hmod) {
            Ok(h) => h,
            Err(e) => {
                let _ = ready.send(Err(format!("keyboard hook: {e}")));
                return;
            }
        };
        let mut ms = install_mouse(hmod);
        let wev = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(foreground_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );

        let mut registered = Vec::new();
        for (action, combo) in &hotkeys {
            match parse_hotkey(combo) {
                Some((mods, vk))
                    if RegisterHotKey(
                        None,
                        action.id(),
                        HOT_KEY_MODIFIERS(mods) | MOD_NOREPEAT,
                        vk,
                    )
                    .is_ok() =>
                {
                    registered.push(*action)
                }
                _ => {
                    let _ = tx.try_send(HookEvent::HotkeyUnavailable(*action));
                }
            }
        }
        let _ = ready.send(Ok(GetCurrentThreadId()));

        // Watchdog: Windows silently drops a low-level hook whose callback timed out (or after a
        // secure-desktop switch such as Win+L). Both hooks stamp every event; if the user produced
        // input well after the last stamp, the hooks are dead and are reinstalled.
        let timer = SetTimer(None, 0, WATCHDOG_PERIOD_MS, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_TIMER && msg.wParam.0 == timer {
                if hooks_look_dead(&shared) {
                    let _ = UnhookWindowsHookEx(kb);
                    if let Some(m) = ms.take() {
                        let _ = UnhookWindowsHookEx(m);
                    }
                    match install_keyboard(hmod) {
                        Ok(h) => {
                            kb = h;
                            ms = install_mouse(hmod);
                            shared.stamp();
                            shared.reinstalls.fetch_add(1, Ordering::Relaxed);
                            shared.reset_buffer();
                        }
                        Err(_) => {
                            // Could not reinstall; keep the stale handle so Unhook at exit is harmless,
                            // and retry on the next tick (stamp stays old).
                        }
                    }
                }
            } else if msg.message == WM_HOTKEY {
                if let Some(a) = HotkeyAction::from_id(msg.wParam.0 as i32) {
                    let _ = tx.try_send(HookEvent::Hotkey(a));
                }
            } else {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        if timer != 0 {
            let _ = KillTimer(None, timer);
        }
        for a in registered {
            let _ = UnregisterHotKey(None, a.id());
        }
        if !wev.is_invalid() {
            let _ = UnhookWinEvent(wev);
        }
        if let Some(m) = ms {
            let _ = UnhookWindowsHookEx(m);
        }
        let _ = UnhookWindowsHookEx(kb);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watchdog_staleness() {
        assert!(!is_stale(1000, 1000));
        assert!(!is_stale(2000, 1000)); // 1 s: normal gap
        assert!(is_stale(9000, 1000)); // 8 s of input the hooks never saw
        assert!(!is_stale(1000, 9000)); // hook stamp newer than input
        // tick-count wraparound
        assert!(is_stale(10_000, u32::MAX - 100));
        assert!(!is_stale(50, u32::MAX - 100));
    }

    #[test]
    fn hotkey_parsing() {
        assert_eq!(
            parse_hotkey("Ctrl+Alt+Space"),
            Some((MOD_CONTROL.0 | MOD_ALT.0, VK_SPACE.0 as u32))
        );
        assert_eq!(
            parse_hotkey("ctrl+alt+shift+m"),
            Some((MOD_CONTROL.0 | MOD_ALT.0 | MOD_SHIFT.0, b'M' as u32))
        );
        assert_eq!(
            parse_hotkey("Ctrl+F5"),
            Some((MOD_CONTROL.0, VK_F5.0 as u32))
        );
        assert_eq!(parse_hotkey("Space"), None); // no modifier ⇒ refused
        assert_eq!(parse_hotkey("Ctrl+Alt"), None);
        assert_eq!(parse_hotkey("Ctrl+Banana"), None);
    }
}
