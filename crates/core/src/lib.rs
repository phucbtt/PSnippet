//! PSnippet core: model, template language, renderer, trigger matcher.
//! Pure logic — no Win32, no I/O.

pub mod config;
pub mod matcher;
pub mod model;
pub mod template;

pub use config::{AppScope, app_entry_matches};
pub use config::{Config, DelimKey, MentionModeCfg};
pub use matcher::{Delim, KeyInput, Match, MatcherConfig, TriggerMatcher};
pub use model::*;
pub use template::{
    FieldKey, FieldSpec, FieldType, FieldValue, FieldValues, MentionChoice, MentionFallback, Node,
    RenderError, RenderSegment, Template, TemplateError, render_plain_text_preview,
};
