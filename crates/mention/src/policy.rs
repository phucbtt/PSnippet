//! Mention modes, capability gating and candidate confidence (spec §6A.2, §6A.10, §12A.4).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MentionMode {
    /// M0: engine inserts text before the tag; user types `@`, picks the member in Zalo, presses Resume.
    #[default]
    Manual,
    /// M1: engine types `@query`; user still picks.
    Assisted,
    /// M2: engine selects a unique, verified candidate. Experimental.
    AutoVerified,
}

/// Evidence level recorded per app profile after QA on a real Zalo build.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MentionCapability {
    #[default]
    Unverified,
    ManualTested,
    AssistedTested,
    AutoTested,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppProfile {
    pub id: String,
    pub display_name: String,
    pub executable_path: Option<String>,
    pub supported_build: Option<String>,
    pub mention_mode: MentionMode,
    pub mention_capability: MentionCapability,
}

impl Default for AppProfile {
    fn default() -> Self {
        Self {
            id: "zalo-pc".into(),
            display_name: "Zalo PC".into(),
            executable_path: None,
            supported_build: None,
            mention_mode: MentionMode::Manual,
            mention_capability: MentionCapability::Unverified,
        }
    }
}

/// Requested mode is clamped to what QA evidence supports; anything unproven degrades to Manual.
/// (MN-36: auto without QA evidence is blocked.)
pub fn effective_mode(requested: MentionMode, cap: MentionCapability) -> MentionMode {
    match requested {
        MentionMode::Manual => MentionMode::Manual,
        MentionMode::Assisted if cap >= MentionCapability::AssistedTested => MentionMode::Assisted,
        MentionMode::AutoVerified if cap >= MentionCapability::AutoTested => {
            MentionMode::AutoVerified
        }
        MentionMode::AutoVerified if cap >= MentionCapability::AssistedTested => {
            MentionMode::Assisted
        }
        _ => MentionMode::Manual,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CandidateConfidence {
    None,
    Low,
    Ambiguous,
    CandidateOnly,
    UiVerified,
}

/// Only `UiVerified` may ever be auto-selected, and only in effective AutoVerified mode.
pub fn auto_select_allowed(mode: MentionMode, conf: CandidateConfidence) -> bool {
    mode == MentionMode::AutoVerified && conf == CandidateConfidence::UiVerified
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MentionVerification {
    /// Only with trustworthy probe evidence for the tested build.
    VerifiedRichMention,
    /// The user picked the member in Zalo and attested via Resume. Not machine-verified.
    UserConfirmed,
    Unverifiable,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_are_clamped_by_evidence() {
        use MentionCapability::*;
        use MentionMode::*;
        assert_eq!(effective_mode(AutoVerified, Unverified), Manual);
        assert_eq!(effective_mode(Assisted, ManualTested), Manual);
        assert_eq!(effective_mode(Assisted, AssistedTested), Assisted);
        assert_eq!(effective_mode(AutoVerified, AssistedTested), Assisted);
        assert_eq!(effective_mode(AutoVerified, AutoTested), AutoVerified);
    }

    #[test]
    fn only_ui_verified_auto_selects() {
        use CandidateConfidence::*;
        for c in [None, Low, Ambiguous, CandidateOnly] {
            assert!(!auto_select_allowed(MentionMode::AutoVerified, c));
        }
        assert!(auto_select_allowed(MentionMode::AutoVerified, UiVerified));
        assert!(!auto_select_allowed(MentionMode::Assisted, UiVerified));
    }
}
