use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::template::{FieldSpec, FieldType, MentionFallback};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TriggerMode {
    #[default]
    Boundary,
    Literal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DelimiterPolicy {
    /// Swallowed delimiter is not re-emitted (default).
    #[default]
    Discard,
    Preserve,
}

/// Per-field metadata override stored next to a snippet (`field_definitions`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldDefinition {
    pub key: String,
    pub field_type: FieldType,
    pub required: bool,
    pub mention_fallback: MentionFallback,
    pub default_value: Option<String>,
}

impl FieldDefinition {
    pub fn from_spec(spec: &FieldSpec) -> Self {
        Self {
            key: spec.key.as_str().to_string(),
            field_type: spec.field_type,
            required: spec.required,
            mention_fallback: spec.mention_fallback,
            default_value: spec.default_value.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snippet {
    pub id: Uuid,
    pub code: String,
    pub title: String,
    pub body: String,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub enabled: bool,
    pub favorite: bool,
    pub trigger_mode: TriggerMode,
    pub delimiter_policy: DelimiterPolicy,
    pub app_scope: Vec<String>,
    /// Overrides (type/required/default) layered on top of what the body declares.
    pub fields: Vec<FieldDefinition>,
}

impl Snippet {
    pub fn new(code: impl Into<String>, title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            code: code.into(),
            title: title.into(),
            body: body.into(),
            category: None,
            tags: vec![],
            enabled: true,
            favorite: false,
            trigger_mode: TriggerMode::Boundary,
            delimiter_policy: DelimiterPolicy::Discard,
            app_scope: vec![],
            fields: vec![],
        }
    }

    /// Parse body and apply stored field overrides.
    pub fn template(&self) -> Result<crate::template::Template, crate::template::TemplateError> {
        let mut t = crate::template::Template::parse(&self.body)?;
        t.apply_overrides(&self.fields);
        Ok(t)
    }
}

/// Validates a trigger code: `^[A-Za-z][A-Za-z0-9_-]{1,31}$`.
pub fn is_valid_code(code: &str) -> bool {
    let mut chars = code.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let rest: Vec<char> = chars.collect();
    first.is_ascii_alphabetic()
        && (1..=31).contains(&rest.len())
        && rest
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplicateCode(pub String);

/// Trigger codes must be unique case-insensitively (spec §3.1/§8).
pub fn find_duplicate_code<'a>(codes: impl IntoIterator<Item = &'a str>) -> Option<DuplicateCode> {
    let mut seen = std::collections::HashSet::new();
    for c in codes {
        if !seen.insert(c.to_lowercase()) {
            return Some(DuplicateCode(c.to_string()));
        }
    }
    None
}
