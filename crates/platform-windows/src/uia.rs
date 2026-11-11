//! UI Automation probe on a dedicated COM MTA worker thread (spec §6.1, §6A.8).
//! Never called from a hook callback. Queries only the *focused element* (no tree sweeps).

use std::sync::mpsc::{self, Sender, SyncSender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, UIA_DocumentControlTypeId, UIA_EditControlTypeId,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FocusProbe {
    pub control_type: i32,
    pub is_password: bool,
    /// `Some(true)` for Edit/Document controls. Other controls ⇒ `Some(false)`.
    pub is_editable: bool,
    /// Fingerprint of the focused element (native hwnd, class, automation id). Not content.
    pub runtime_id: Option<String>,
}

struct Req {
    reply: SyncSender<Option<FocusProbe>>,
}

fn worker() -> &'static Mutex<Sender<Req>> {
    static W: OnceLock<Mutex<Sender<Req>>> = OnceLock::new();
    W.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Req>();
        std::thread::Builder::new()
            .name("zs-uia".into())
            .spawn(move || {
                // COM MTA, no window ownership (spec §6.1).
                let uia: Option<IUIAutomation> = unsafe {
                    let _ = CoInitializeEx(None, COINIT_MULTITHREADED).ok();
                    CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()
                };
                for req in rx {
                    let out = uia.as_ref().and_then(probe_once);
                    let _ = req.reply.send(out);
                }
            })
            .ok();
        Mutex::new(tx)
    })
}

fn probe_once(uia: &IUIAutomation) -> Option<FocusProbe> {
    unsafe {
        let el = uia.GetFocusedElement().ok()?;
        let ct = el.CurrentControlType().ok()?.0;
        let is_password = el.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(false);
        let hwnd = el
            .CurrentNativeWindowHandle()
            .map(|h| h.0 as isize)
            .unwrap_or(0);
        let class = el
            .CurrentClassName()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let aid = el
            .CurrentAutomationId()
            .map(|s| s.to_string())
            .unwrap_or_default();
        Some(FocusProbe {
            control_type: ct,
            is_password,
            is_editable: ct == UIA_EditControlTypeId.0 || ct == UIA_DocumentControlTypeId.0,
            runtime_id: Some(format!("{hwnd}|{ct}|{class}|{aid}")),
        })
    }
}

/// Probe the focused element with a timeout. `None` = could not determine (treat as Unverifiable).
pub fn probe_focus(timeout: Duration) -> Option<FocusProbe> {
    let (rtx, rrx) = mpsc::sync_channel(1);
    worker().lock().ok()?.send(Req { reply: rtx }).ok()?;
    rrx.recv_timeout(timeout).ok().flatten()
}
