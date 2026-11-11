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
    /// Executable allowlist (path or file name). Empty ⇒ hotstrings are inactive everywhere.
    pub allowed_apps: Vec<String>,
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
            allowed_apps: vec![],
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

/// Heuristic for the "Zalo only" scope: the executable file name contains "zalo".
pub fn is_zalo_entry(entry: &str) -> bool {
    let name = entry.rsplit(['\\', '/']).next().unwrap_or(entry);
    name.to_ascii_lowercase().contains("zalo")
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(c.strict_focus && c.allowed_apps.is_empty());
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
