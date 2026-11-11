//! User configuration (spec §5.3, §5A.3). Stored as JSON in the `settings` table.
//! Defaults are the safe ones: Space only, allowlist + fail-closed, manual mention, no PII persistence.

use serde::{Deserialize, Serialize};

use crate::matcher::{Delim, MatcherConfig};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelimKey {
    Space,
    Tab,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MentionModeCfg {
    #[default]
    Manual,
    Assisted,
    AutoVerified,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub hotstring_enabled: bool,
    pub trigger_delimiters: Vec<DelimKey>,
    pub case_sensitive: bool,
    pub trigger_literal_mode: bool,
    pub require_word_boundary: bool,
    pub palette_hotkey: String,
    /// Global hotkey that turns hotstring expansion on/off.
    pub toggle_hotkey: String,
    /// `true` ⇒ fail closed when the editable context cannot be verified. `false` = compatibility mode.
    pub strict_focus: bool,
    /// `true` (default) ⇒ hotstrings work in every app except `blocked_apps`.
    /// `false` ⇒ only in `allowed_apps` (empty ⇒ inactive everywhere).
    pub allow_all_apps: bool,
    /// Executable allowlist (path or file name), used when `allow_all_apps` is false.
    pub allowed_apps: Vec<String>,
    /// Apps where hotstrings never fire (always applied, in both modes).
    pub blocked_apps: Vec<String>,
    /// Tray "Scope": `true` => only allowlist entries that look like Zalo are active.
    pub zalo_only: bool,

    pub mention_mode: MentionModeCfg,
    pub mention_resume_hotkey: String,
    pub mention_cancel_hotkey: String,
    pub require_same_chat: bool,
    pub session_timeout_seconds: u64,
    pub ui_probe_timeout_ms: u64,

    /// "best_effort" | "never"
    pub clipboard_restore: String,
    pub live_preview: bool,
    /// UniKey-style macros: trigger + Space immediately expands in place (fields stay as `[Label]`),
    /// no form. `false` ⇒ snippets with fields open the form.
    pub instant_expand: bool,
    pub preserve_delimiter: bool,

    pub remember_field_values: bool,
    pub autostart: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hotstring_enabled: true,
            trigger_delimiters: vec![DelimKey::Space],
            case_sensitive: false,
            trigger_literal_mode: false,
            require_word_boundary: true,
            palette_hotkey: "Ctrl+Alt+Space".into(),
            toggle_hotkey: "Ctrl+Alt+P".into(),
            strict_focus: true,
            allow_all_apps: true,
            allowed_apps: vec![],
            // Password managers: never expand there, even if their fields are not reported as password.
            blocked_apps: [
                "KeePass.exe",
                "KeePassXC.exe",
                "1Password.exe",
                "Bitwarden.exe",
            ]
            .map(String::from)
            .to_vec(),
            zalo_only: false,
            mention_mode: MentionModeCfg::Manual,
            mention_resume_hotkey: "Ctrl+Alt+Shift+M".into(),
            mention_cancel_hotkey: "Ctrl+Alt+Shift+X".into(),
            require_same_chat: true,
            session_timeout_seconds: 120,
            ui_probe_timeout_ms: 1500,
            clipboard_restore: "best_effort".into(),
            live_preview: true,
            instant_expand: true,
            preserve_delimiter: false,
            remember_field_values: false,
            autostart: false,
        }
    }
}

impl Config {
    /// The scope the keyboard hook enforces.
    pub fn app_scope(&self) -> AppScope {
        AppScope {
            all: self.allow_all_apps,
            allowed: self.effective_allowed_apps(),
            blocked: self.blocked_apps.clone(),
        }
    }

    /// The allowlist the hook actually enforces (never wider than `allowed_apps`).
    pub fn effective_allowed_apps(&self) -> Vec<String> {
        self.allowed_apps
            .iter()
            .filter(|a| !self.zalo_only || is_zalo_entry(a))
            .cloned()
            .collect()
    }

    pub fn matcher_config(&self) -> MatcherConfig {
        MatcherConfig {
            delimiters: self
                .trigger_delimiters
                .iter()
                .map(|d| match d {
                    DelimKey::Space => Delim::Space,
                    DelimKey::Tab => Delim::Tab,
                })
                .collect(),
            case_sensitive: self.case_sensitive,
            literal_mode: self.trigger_literal_mode,
            require_word_boundary: self.require_word_boundary,
        }
    }
}

/// Where hotstrings may fire. The block list always wins.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppScope {
    pub all: bool,
    pub allowed: Vec<String>,
    pub blocked: Vec<String>,
}

impl AppScope {
    pub fn permits(&self, exe_path: &str) -> bool {
        if self.blocked.iter().any(|b| app_entry_matches(exe_path, b)) {
            return false;
        }
        self.all || self.allowed.iter().any(|a| app_entry_matches(exe_path, a))
    }
}

/// Does `exe_path` match a list entry (full path or bare file name, case-insensitive)?
pub fn app_entry_matches(exe_path: &str, entry: &str) -> bool {
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

/// Heuristic for the "Zalo only" scope: the executable file name contains "zalo".
pub fn is_zalo_entry(entry: &str) -> bool {
    let name = entry.rsplit(['\\', '/']).next().unwrap_or(entry);
    name.to_ascii_lowercase().contains("zalo")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_scope_all_apps_except_blocked() {
        let c = Config::default();
        let s = c.app_scope();
        assert!(s.permits(r"C:\Windows\System32\notepad.exe"));
        assert!(s.permits(r"C:\Users\a\AppData\Local\Programs\Zalo\Zalo.exe"));
        assert!(!s.permits(r"C:\Program Files\KeePassXC\KeePassXC.exe"));
    }

    #[test]
    fn app_scope_allowlist_mode() {
        let mut c = Config {
            allow_all_apps: false,
            ..Config::default()
        };
        assert!(!c.app_scope().permits(r"C:\x\notepad.exe")); // empty allowlist ⇒ nothing
        c.allowed_apps.push("notepad.exe".into());
        c.blocked_apps.push(r"C:\x\notepad.exe".into());
        assert!(!c.app_scope().permits(r"C:\x\notepad.exe")); // block list wins
        assert!(c.app_scope().permits(r"D:\y\notepad.exe"));
    }

    #[test]
    fn zalo_only_scope_filters_allowlist() {
        let mut c = Config {
            allowed_apps: vec![
                r"C:\Users\a\AppData\Local\Programs\Zalo\Zalo.exe".into(),
                "notepad.exe".into(),
                r"C:\tools\zalo-helper\notepad.exe".into(), // directory name must not count
            ],
            ..Config::default()
        };
        assert_eq!(c.effective_allowed_apps().len(), 3);
        c.zalo_only = true;
        assert_eq!(c.effective_allowed_apps().len(), 1);
        c.allowed_apps.clear();
        assert!(c.effective_allowed_apps().is_empty());
    }

    #[test]
    fn defaults_match_spec_safety_posture() {
        let c = Config::default();
        assert_eq!(c.trigger_delimiters, [DelimKey::Space]); // never Enter
        assert!(c.strict_focus && c.allowed_apps.is_empty() && c.allow_all_apps);
        assert!(!c.remember_field_values && !c.autostart);
        assert_eq!(c.mention_mode, MentionModeCfg::Manual);
        assert_eq!(c.matcher_config().delimiters, [Delim::Space]);
    }

    #[test]
    fn partial_json_fills_defaults() {
        let c: Config = serde_json::from_str(r#"{"strict_focus":false}"#).unwrap();
        assert!(!c.strict_focus);
        assert_eq!(c.session_timeout_seconds, 120);
    }
}
