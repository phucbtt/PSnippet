//! Clipboard handling (spec §6.2): CF_UNICODETEXT write, sequence-number guarded best-effort restore,
//! exclusion from Windows clipboard history/cloud sync so customer text does not leak there.

use std::time::Duration;

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::core::w;

use crate::{PlatformError, Result};

const MAX_SNAPSHOT_BYTES: usize = 32 * 1024 * 1024;

struct ClipGuard;
impl Drop for ClipGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

fn open() -> Result<ClipGuard> {
    for _ in 0..20 {
        if unsafe { OpenClipboard(Some(HWND::default())) }.is_ok() {
            return Ok(ClipGuard);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Err(PlatformError::ClipboardBusy)
}

pub fn sequence_number() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

/// Allocate movable global memory holding `bytes` (ownership passes to the clipboard on success).
unsafe fn alloc_global(bytes: &[u8]) -> Result<HGLOBAL> {
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1))?;
        let p = GlobalLock(h);
        if p.is_null() {
            let _ = GlobalFree(Some(h));
            return Err(PlatformError::Other("GlobalLock failed".into()));
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p as *mut u8, bytes.len());
        let _ = GlobalUnlock(h);
        Ok(h)
    }
}

/// `\n` → `\r\n`, NUL-terminated UTF-16LE bytes.
fn utf16_bytes(text: &str) -> Vec<u8> {
    let normalized = text.replace("\r\n", "\n").replace('\n', "\r\n");
    normalized
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

unsafe fn set_raw(format: u32, bytes: &[u8]) -> Result<()> {
    unsafe {
        let h = alloc_global(bytes)?;
        if SetClipboardData(format, Some(HANDLE(h.0))).is_err() {
            let _ = GlobalFree(Some(h));
            return Err(PlatformError::Other("SetClipboardData failed".into()));
        }
        Ok(())
    }
}

/// Replace clipboard content with `text`, flagged to stay out of clipboard history/cloud sync.
/// Returns the clipboard sequence number right after our write.
pub fn set_text(text: &str) -> Result<u32> {
    let _g = open()?;
    unsafe {
        EmptyClipboard()?;
        set_raw(CF_UNICODETEXT.0 as u32, &utf16_bytes(text))?;
        // Keep customer text out of Win+V history and cloud clipboard.
        let no_history = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
        let no_cloud = RegisterClipboardFormatW(w!("CanUploadToCloudClipboard"));
        let no_monitor =
            RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
        let zero = 0u32.to_le_bytes();
        for f in [no_history, no_cloud] {
            if f != 0 {
                let _ = set_raw(f, &zero);
            }
        }
        if no_monitor != 0 {
            let _ = set_raw(no_monitor, &zero);
        }
    }
    drop(_g);
    Ok(sequence_number())
}

pub fn get_text() -> Option<String> {
    let _g = open().ok()?;
    unsafe {
        let h = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
        let hg = HGLOBAL(h.0);
        let p = GlobalLock(hg) as *const u16;
        if p.is_null() {
            return None;
        }
        let max = GlobalSize(hg) / 2;
        let mut n = 0;
        while n < max && *p.add(n) != 0 {
            n += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
        let _ = GlobalUnlock(hg);
        Some(s)
    }
}

/// Best-effort copy of memory-backed clipboard formats. GDI-handle formats (CF_BITMAP, metafiles,
/// palettes) and owner-display formats are skipped — restoring *every* format is not guaranteed (§6.2).
pub struct Snapshot {
    items: Vec<(u32, Vec<u8>)>,
    pub complete: bool,
}

pub fn snapshot() -> Result<Snapshot> {
    let _g = open()?;
    let mut items = Vec::new();
    let mut total = 0usize;
    let mut complete = true;
    unsafe {
        let mut fmt = EnumClipboardFormats(0);
        while fmt != 0 {
            // 2=CF_BITMAP 3=CF_METAFILEPICT 9=CF_PALETTE 14=CF_ENHMETAFILE 0x80..0x8F owner display / private GDI
            let skip = matches!(fmt, 2 | 3 | 9 | 14)
                || (0x80..=0x8F).contains(&fmt)
                || (0x200..=0x2FF).contains(&fmt);
            if skip {
                complete = false;
            } else if let Ok(h) = GetClipboardData(fmt) {
                let hg = HGLOBAL(h.0);
                let size = GlobalSize(hg);
                if size == 0 || total + size > MAX_SNAPSHOT_BYTES {
                    complete = false;
                } else {
                    let p = GlobalLock(hg) as *const u8;
                    if p.is_null() {
                        complete = false;
                    } else {
                        items.push((fmt, std::slice::from_raw_parts(p, size).to_vec()));
                        total += size;
                        let _ = GlobalUnlock(hg);
                    }
                }
            } else {
                complete = false;
            }
            fmt = EnumClipboardFormats(fmt);
        }
    }
    Ok(Snapshot { items, complete })
}

/// Restore only if the clipboard still holds *our* temporary content (sequence number unchanged),
/// so a clipboard the user changed in the meantime is never overwritten (IT-17, MN-26).
pub fn restore_if_unchanged(snap: &Snapshot, our_seq: u32) -> bool {
    if sequence_number() != our_seq || snap.items.is_empty() {
        return false;
    }
    let Ok(_g) = open() else { return false };
    unsafe {
        if EmptyClipboard().is_err() {
            return false;
        }
        for (fmt, bytes) in &snap.items {
            let _ = set_raw(*fmt, bytes);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_normalizes_newlines_and_terminates() {
        let b = utf16_bytes("a\nb\r\nc");
        let units: Vec<u16> = b
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(
            String::from_utf16_lossy(&units[..units.len() - 1]),
            "a\r\nb\r\nc"
        );
        assert_eq!(*units.last().unwrap(), 0);
    }

    #[test]
    fn utf16_keeps_vietnamese_and_emoji() {
        let b = utf16_bytes("Hiền 😀");
        let units: Vec<u16> = b
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(
            String::from_utf16_lossy(&units[..units.len() - 1]),
            "Hiền 😀"
        );
    }
}
