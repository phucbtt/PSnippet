//! Target context & chat identity guard (spec §6A.7, §8A.3). RAM-only; never serialized.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetContext {
    pub hwnd: isize,
    pub pid: u32,
    /// Process creation time (FILETIME ticks) — guards against PID/HWND reuse.
    pub process_start_time: u64,
    pub executable_path: String,
    /// UIA runtime id (or similar) of the focused editable control, if exposed.
    pub focus_signature: Option<String>,
    /// Fingerprint of chat header/title, if UIA exposes it. Never chat content.
    pub chat_signature: Option<String>,
    /// Bumped by the platform layer whenever foreground/focus changes.
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextVerdict {
    Same,
    /// Provably different window/process/chat/focus.
    Changed,
    /// Could not prove it is the same chat/editor.
    Unverifiable,
}

/// Compare the snapshot taken at trigger time with the live context.
///
/// * Window/process identity or generation mismatch ⇒ `Changed` (always).
/// * Both signatures present and different ⇒ `Changed`.
/// * Chat signature missing on either side ⇒ `Unverifiable` (same PID is not enough, §6A.7).
pub fn check_context(snapshot: &TargetContext, live: &TargetContext) -> ContextVerdict {
    if snapshot.hwnd != live.hwnd
        || snapshot.pid != live.pid
        || snapshot.process_start_time != live.process_start_time
        || !snapshot
            .executable_path
            .eq_ignore_ascii_case(&live.executable_path)
        || snapshot.generation != live.generation
    {
        return ContextVerdict::Changed;
    }
    if let (Some(a), Some(b)) = (&snapshot.focus_signature, &live.focus_signature)
        && a != b
    {
        return ContextVerdict::Changed;
    }
    match (&snapshot.chat_signature, &live.chat_signature) {
        (Some(a), Some(b)) if a == b => ContextVerdict::Same,
        (Some(_), Some(_)) => ContextVerdict::Changed,
        _ => ContextVerdict::Unverifiable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> TargetContext {
        TargetContext {
            hwnd: 100,
            pid: 7,
            process_start_time: 42,
            executable_path: r"C:\Zalo\Zalo.exe".into(),
            focus_signature: Some("f1".into()),
            chat_signature: Some("Nhóm A".into()),
            generation: 3,
        }
    }

    #[test]
    fn identical_is_same() {
        assert_eq!(check_context(&ctx(), &ctx()), ContextVerdict::Same);
    }

    #[test]
    fn chat_switch_is_changed() {
        let mut live = ctx();
        live.chat_signature = Some("Nhóm B".into());
        assert_eq!(check_context(&ctx(), &live), ContextVerdict::Changed);
    }

    #[test]
    fn pid_reuse_and_generation_detected() {
        let mut live = ctx();
        live.process_start_time = 43;
        assert_eq!(check_context(&ctx(), &live), ContextVerdict::Changed);
        let mut live = ctx();
        live.generation = 4;
        assert_eq!(check_context(&ctx(), &live), ContextVerdict::Changed);
        let mut live = ctx();
        live.hwnd = 101;
        assert_eq!(check_context(&ctx(), &live), ContextVerdict::Changed);
    }

    #[test]
    fn missing_chat_signature_is_unverifiable_not_same() {
        let mut a = ctx();
        a.chat_signature = None;
        let mut b = ctx();
        b.chat_signature = None;
        assert_eq!(check_context(&a, &b), ContextVerdict::Unverifiable);
    }

    #[test]
    fn focus_signature_change_is_changed() {
        let mut live = ctx();
        live.focus_signature = Some("f2".into());
        assert_eq!(check_context(&ctx(), &live), ContextVerdict::Changed);
    }
}
