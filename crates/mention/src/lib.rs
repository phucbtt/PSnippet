//! Mention engine (spec §6A): target-context guard, segmented-composer session state machine,
//! candidate-confidence policy. Sans-IO: the session consumes `Event`s and emits `Action`s;
//! the platform layer performs them. There is deliberately NO `Send`/`Enter` action.

pub mod context;
pub mod policy;
pub mod session;

pub use context::{ContextVerdict, TargetContext, check_context};
pub use policy::{
    AppProfile, CandidateConfidence, MentionCapability, MentionMode, MentionVerification,
    auto_select_allowed, effective_mode,
};
pub use session::{
    Action, ComposeSession, ComposeState, ErrorCode, Event, IllegalTransition, Status,
};
