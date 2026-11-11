//! `.zsnip` JSON export/import (spec §8.1, §8A.2). v1 files import as v2.

use serde::{Deserialize, Serialize};
use zs_core::{
    FieldDefinition, FieldType, MentionFallback, Snippet, Template, find_duplicate_code,
    is_valid_code,
};

pub const FORMAT: &str = "psnippet.export";
/// Files exported before the rename are still accepted.
const LEGACY_FORMAT: &str = "zalosnippet.export";
pub const CURRENT_SCHEMA: u32 = 2;

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not a PSnippet export (format = {0:?})")]
    WrongFormat(String),
    #[error("unsupported schema_version {0} (max {CURRENT_SCHEMA})")]
    UnsupportedVersion(u32),
    #[error("snippet '{code}': {reason}")]
    InvalidSnippet { code: String, reason: String },
}

#[derive(Serialize, Deserialize)]
struct File {
    format: String,
    schema_version: u32,
    #[serde(default)]
    exported_at: String,
    snippets: Vec<Item>,
}

#[derive(Serialize, Deserialize)]
struct Item {
    code: String,
    #[serde(default)]
    title: String,
    body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    category: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
    #[serde(default = "yes")]
    enabled: bool,
    #[serde(default)]
    favorite: bool,
    #[serde(default)]
    fields: Vec<Fld>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    app_scope: Vec<String>,
}

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize)]
struct Fld {
    key: String,
    #[serde(rename = "type", default = "text")]
    ty: String,
    #[serde(default = "yes")]
    required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mention_fallback: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_value: Option<String>,
}

fn text() -> String {
    "text".into()
}

pub fn export_json(snippets: &[Snippet], exported_at: &str) -> Result<String, ExportError> {
    let items = snippets
        .iter()
        .map(|s| Item {
            code: s.code.clone(),
            title: s.title.clone(),
            body: s.body.clone(),
            category: s.category.clone(),
            tags: s.tags.clone(),
            enabled: s.enabled,
            favorite: s.favorite,
            fields: s
                .fields
                .iter()
                .map(|f| Fld {
                    key: f.key.clone(),
                    ty: match f.field_type {
                        FieldType::Text => "text",
                        FieldType::Multiline => "multiline",
                        FieldType::Mention => "mention",
                    }
                    .into(),
                    required: f.required,
                    mention_fallback: (f.field_type == FieldType::Mention).then(|| {
                        match f.mention_fallback {
                            MentionFallback::RequireReal => "require_real",
                            MentionFallback::AskTextOrMention => "ask_text_or_mention",
                        }
                        .to_string()
                    }),
                    default_value: f.default_value.clone(),
                })
                .collect(),
            app_scope: s.app_scope.clone(),
        })
        .collect();
    let file = File {
        format: FORMAT.into(),
        schema_version: CURRENT_SCHEMA,
        exported_at: exported_at.into(),
        snippets: items,
    };
    Ok(serde_json::to_string_pretty(&file)?)
}

pub fn import_json(input: &str) -> Result<Vec<Snippet>, ExportError> {
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let file: File = serde_json::from_str(input)?;
    if file.format != FORMAT && file.format != LEGACY_FORMAT {
        return Err(ExportError::WrongFormat(file.format));
    }
    if file.schema_version == 0 || file.schema_version > CURRENT_SCHEMA {
        return Err(ExportError::UnsupportedVersion(file.schema_version));
    }
    if let Some(d) = find_duplicate_code(file.snippets.iter().map(|s| s.code.as_str())) {
        return Err(ExportError::InvalidSnippet {
            code: d.0,
            reason: "duplicate code in file".into(),
        });
    }
    let mut out = Vec::new();
    for it in file.snippets {
        let bad = |reason: String| ExportError::InvalidSnippet {
            code: it.code.clone(),
            reason,
        };
        if !is_valid_code(&it.code) {
            return Err(bad("invalid trigger code".into()));
        }
        Template::parse(&it.body).map_err(|e| bad(e.to_string()))?;
        let mut s = Snippet::new(
            it.code.clone(),
            if it.title.is_empty() {
                it.code.clone()
            } else {
                it.title.clone()
            },
            it.body.clone(),
        );
        s.category = it.category.clone();
        s.tags = it.tags.clone();
        s.enabled = it.enabled;
        s.favorite = it.favorite;
        s.app_scope = it.app_scope.clone();
        for f in &it.fields {
            let field_type = match f.ty.as_str() {
                "text" => FieldType::Text,
                "multiline" | "textarea" => FieldType::Multiline, // "textarea" is the v1 spelling
                "mention" => FieldType::Mention,
                other => return Err(bad(format!("unknown field type '{other}'"))),
            };
            let mention_fallback = match f.mention_fallback.as_deref() {
                None | Some("require_real") => MentionFallback::RequireReal,
                Some("ask_text_or_mention") => MentionFallback::AskTextOrMention,
                Some(o) => return Err(bad(format!("unknown mention_fallback '{o}'"))),
            };
            s.fields.push(FieldDefinition {
                key: f.key.clone(),
                field_type,
                required: f.required,
                mention_fallback,
                default_value: f.default_value.clone(),
            });
        }
        out.push(s);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seeds::seed_snippets;

    #[test]
    fn v2_round_trip_keeps_unicode_and_fields() {
        let mut seeds = seed_snippets();
        seeds[0].fields.push(FieldDefinition {
            key: "Tên".into(),
            field_type: FieldType::Mention,
            required: true,
            mention_fallback: MentionFallback::AskTextOrMention,
            default_value: None,
        });
        let json = export_json(&seeds, "2026-11-11T00:00:00Z").unwrap();
        let back = import_json(&json).unwrap();
        assert_eq!(back.len(), 7);
        for (a, b) in seeds.iter().zip(&back) {
            assert_eq!(
                (&a.code, &a.body, &a.fields, &a.category),
                (&b.code, &b.body, &b.fields, &b.category)
            );
        }
    }

    #[test]
    fn imports_v1_sample_from_spec() {
        let v1 = r#"{"format":"zalosnippet.export","schema_version":1,"exported_at":"2026-11-11T00:00:00Z",
          "snippets":[{"code":"ztn","title":"Tiếp nhận hỗ trợ","body":"chị [Tên]. [Nội dung]","category":"Tiếp nhận",
          "tags":["Zalo","CS"],"enabled":true,"fields":[{"key":"Tên","type":"text","required":true},
          {"key":"Nội dung","type":"textarea","required":true}]}]}"#;
        let s = import_json(v1).unwrap();
        assert_eq!(s[0].fields[1].field_type, FieldType::Multiline);
        assert_eq!(s[0].tags, ["Zalo", "CS"]);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(matches!(import_json("{}"), Err(ExportError::Json(_))));
        assert!(matches!(
            import_json(r#"{"format":"x","schema_version":1,"snippets":[]}"#),
            Err(ExportError::WrongFormat(_))
        ));
        assert!(matches!(
            import_json(r#"{"format":"zalosnippet.export","schema_version":9,"snippets":[]}"#),
            Err(ExportError::UnsupportedVersion(9))
        ));
        let bad_body = r#"{"format":"zalosnippet.export","schema_version":2,"snippets":[{"code":"ab","body":"[oops"}]}"#;
        assert!(matches!(
            import_json(bad_body),
            Err(ExportError::InvalidSnippet { .. })
        ));
        let dup = r#"{"format":"zalosnippet.export","schema_version":2,"snippets":[{"code":"ab","body":"x"},{"code":"AB","body":"y"}]}"#;
        assert!(matches!(
            import_json(dup),
            Err(ExportError::InvalidSnippet { .. })
        ));
    }
}
