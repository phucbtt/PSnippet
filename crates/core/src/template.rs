//! Template language (spec §4, §4A): placeholders `[Label]`, `[Label:text|multiline|mention[?]]`,
//! escapes `\[` `\]` `\\`. Templates are data, never code.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::model::FieldDefinition;

const MAX_LABEL_CHARS: usize = 64;

/// Canonical field key: Unicode NFC of the label (diacritics are significant).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FieldKey(String);

impl FieldKey {
    pub fn new(label: &str) -> Self {
        Self(label.nfc().collect())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for FieldKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    #[default]
    Text,
    Multiline,
    Mention,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MentionFallback {
    /// `[X:mention]` — a real tag is mandatory, never silently degraded to text.
    #[default]
    #[serde(rename = "require_real")]
    RequireReal,
    /// `[X:mention?]` — user may pick real tag or plain text.
    #[serde(rename = "ask_text_or_mention")]
    AskTextOrMention,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldSpec {
    pub key: FieldKey,
    /// Label as shown in UI (NFC).
    pub label: String,
    pub field_type: FieldType,
    pub required: bool,
    pub mention_fallback: MentionFallback,
    pub default_value: Option<String>,
    /// True when the body itself declared a type (`[X:mention]`); body declarations win over overrides.
    pub explicit_type: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Text(String),
    Slot(FieldKey),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    pub nodes: Vec<Node>,
    /// Unique fields in order of first appearance.
    pub fields: Vec<FieldSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("E_PARSER_INVALID {line}:{col}: {kind}")]
pub struct TemplateError {
    pub line: usize,
    pub col: usize,
    pub kind: TemplateErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TemplateErrorKind {
    #[error("placeholder not closed")]
    Unclosed,
    #[error("nested '[' inside placeholder")]
    Nested,
    #[error("empty placeholder label")]
    EmptyLabel,
    #[error("label longer than {MAX_LABEL_CHARS} characters")]
    LabelTooLong,
    #[error("unknown field type '{0}'")]
    UnknownType(String),
    #[error("field '{0}' declared with conflicting types")]
    Conflict(String),
}

impl Template {
    pub fn parse(body: &str) -> Result<Self, TemplateError> {
        let chars: Vec<char> = body.chars().collect();
        let mut nodes: Vec<Node> = Vec::new();
        let mut text = String::new();
        let mut fields: Vec<FieldSpec> = Vec::new();
        let (mut i, mut line, mut col) = (0usize, 1usize, 1usize);

        let err = |line, col, kind| TemplateError { line, col, kind };

        while i < chars.len() {
            let c = chars[i];
            match c {
                '\\' if matches!(chars.get(i + 1), Some('[' | ']' | '\\')) => {
                    text.push(chars[i + 1]);
                    i += 2;
                    col += 2;
                }
                '[' => {
                    let (start_line, start_col) = (line, col);
                    let mut j = i + 1;
                    loop {
                        match chars.get(j) {
                            None | Some('\n') | Some('\r') => {
                                return Err(err(
                                    start_line,
                                    start_col,
                                    TemplateErrorKind::Unclosed,
                                ));
                            }
                            Some('[') => {
                                return Err(err(start_line, start_col, TemplateErrorKind::Nested));
                            }
                            Some(']') => break,
                            _ => j += 1,
                        }
                    }
                    let inner: String = chars[i + 1..j].iter().collect();
                    let decl =
                        parse_placeholder(&inner).map_err(|k| err(start_line, start_col, k))?;
                    let key =
                        register(&mut fields, decl).map_err(|k| err(start_line, start_col, k))?;
                    if !text.is_empty() {
                        nodes.push(Node::Text(std::mem::take(&mut text)));
                    }
                    nodes.push(Node::Slot(key));
                    col += j + 1 - i;
                    i = j + 1;
                }
                '\n' => {
                    text.push('\n');
                    line += 1;
                    col = 1;
                    i += 1;
                }
                _ => {
                    text.push(c);
                    col += 1;
                    i += 1;
                }
            }
        }
        if !text.is_empty() {
            nodes.push(Node::Text(text));
        }
        Ok(Self { nodes, fields })
    }

    pub fn field(&self, key: &FieldKey) -> Option<&FieldSpec> {
        self.fields.iter().find(|f| &f.key == key)
    }

    /// Layer stored metadata on top of body declarations (§4A.1: body-explicit type wins).
    pub fn apply_overrides(&mut self, defs: &[FieldDefinition]) {
        for def in defs {
            let key = FieldKey::new(&def.key);
            if let Some(f) = self.fields.iter_mut().find(|f| f.key == key) {
                if !f.explicit_type {
                    f.field_type = def.field_type;
                    f.mention_fallback = def.mention_fallback;
                }
                f.required = def.required;
                f.default_value = def.default_value.clone();
            }
        }
    }

    /// Required fields that have no usable value.
    pub fn missing_required(&self, values: &FieldValues) -> Vec<FieldKey> {
        self.fields
            .iter()
            .filter(|f| f.required && values.effective(f).trim().is_empty())
            .map(|f| f.key.clone())
            .collect()
    }

    /// UniKey-style macro expansion: every field is left as its literal `[Label]` placeholder so the
    /// user can overwrite it in place. Never produces Mention segments (no fake tags).
    pub fn render_placeholders(&self) -> Vec<RenderSegment> {
        let mut out: Vec<RenderSegment> = Vec::new();
        for node in &self.nodes {
            match node {
                Node::Text(t) => push_text(&mut out, t),
                Node::Slot(key) => {
                    let label = self.field(key).map_or(key.as_str(), |f| f.label.as_str());
                    push_text(&mut out, &format!("[{label}]"));
                }
            }
        }
        out
    }

    /// Render to an ordered plan. Mention fields become `RenderSegment::Mention` — they are NEVER
    /// turned into `@name` text unless the user explicitly chose plain text for a `mention?` field.
    pub fn render(&self, values: &FieldValues) -> Result<Vec<RenderSegment>, RenderError> {
        if let Some(k) = self.missing_required(values).into_iter().next() {
            return Err(RenderError::MissingRequired(k));
        }
        let mut out: Vec<RenderSegment> = Vec::new();
        for node in &self.nodes {
            match node {
                Node::Text(t) => push_text(&mut out, t),
                Node::Slot(key) => {
                    let spec = self.field(key).expect("slot refers to registered field");
                    let value = values.effective(spec);
                    match spec.field_type {
                        FieldType::Text | FieldType::Multiline => push_text(&mut out, &value),
                        FieldType::Mention => {
                            let as_text = spec.mention_fallback
                                == MentionFallback::AskTextOrMention
                                && values.choice(key) == MentionChoice::PlainText;
                            if as_text || value.trim().is_empty() {
                                push_text(&mut out, &value);
                            } else {
                                let name = single_line(&value);
                                out.push(RenderSegment::Mention {
                                    field_key: key.clone(),
                                    query: name.clone(),
                                    display_name: name,
                                    required: spec.mention_fallback == MentionFallback::RequireReal,
                                });
                            }
                        }
                    }
                }
            }
        }
        Ok(out)
    }
}

fn single_line(s: &str) -> String {
    s.split(['\r', '\n'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn push_text(out: &mut Vec<RenderSegment>, t: &str) {
    if t.is_empty() {
        return;
    }
    if let Some(RenderSegment::Text(prev)) = out.last_mut() {
        prev.push_str(t);
    } else {
        out.push(RenderSegment::Text(t.to_string()));
    }
}

struct Decl {
    label: String,
    typed: Option<(FieldType, MentionFallback)>,
}

fn parse_placeholder(inner: &str) -> Result<Decl, TemplateErrorKind> {
    let (label_raw, ty_raw) = match inner.split_once(':') {
        Some((l, t)) => (l, Some(t)),
        None => (inner, None),
    };
    let label: String = label_raw.trim().nfc().collect();
    if label.is_empty() {
        return Err(TemplateErrorKind::EmptyLabel);
    }
    if label.chars().count() > MAX_LABEL_CHARS {
        return Err(TemplateErrorKind::LabelTooLong);
    }
    let typed = match ty_raw.map(str::trim) {
        None => None,
        Some("text") => Some((FieldType::Text, MentionFallback::RequireReal)),
        Some("multiline") => Some((FieldType::Multiline, MentionFallback::RequireReal)),
        Some("mention") => Some((FieldType::Mention, MentionFallback::RequireReal)),
        Some("mention?") => Some((FieldType::Mention, MentionFallback::AskTextOrMention)),
        Some(other) => return Err(TemplateErrorKind::UnknownType(other.to_string())),
    };
    Ok(Decl { label, typed })
}

fn register(fields: &mut Vec<FieldSpec>, d: Decl) -> Result<FieldKey, TemplateErrorKind> {
    let key = FieldKey::new(&d.label);
    match fields.iter_mut().find(|f| f.key == key) {
        None => fields.push(FieldSpec {
            key: key.clone(),
            label: d.label,
            field_type: d.typed.map_or(FieldType::Text, |t| t.0),
            required: true,
            mention_fallback: d.typed.map_or(MentionFallback::RequireReal, |t| t.1),
            default_value: None,
            explicit_type: d.typed.is_some(),
        }),
        Some(existing) => {
            if let Some((ty, fb)) = d.typed {
                if existing.explicit_type {
                    let differs = existing.field_type != ty
                        || (ty == FieldType::Mention && existing.mention_fallback != fb);
                    if differs {
                        return Err(TemplateErrorKind::Conflict(existing.label.clone()));
                    }
                } else {
                    existing.field_type = ty;
                    existing.mention_fallback = fb;
                    existing.explicit_type = true;
                }
            }
        }
    }
    Ok(key)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MentionChoice {
    #[default]
    Tag,
    PlainText,
}

#[derive(Clone, Debug, Default)]
pub struct FieldValue {
    pub text: String,
    pub choice: MentionChoice,
}

#[derive(Clone, Debug, Default)]
pub struct FieldValues(HashMap<FieldKey, FieldValue>);

impl FieldValues {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set(&mut self, key: &FieldKey, text: impl Into<String>) {
        self.0.entry(key.clone()).or_default().text = text.into();
    }
    pub fn set_choice(&mut self, key: &FieldKey, choice: MentionChoice) {
        self.0.entry(key.clone()).or_default().choice = choice;
    }
    pub fn text(&self, key: &FieldKey) -> &str {
        self.0.get(key).map_or("", |v| v.text.as_str())
    }
    pub fn choice(&self, key: &FieldKey) -> MentionChoice {
        self.0.get(key).map(|v| v.choice).unwrap_or_default()
    }
    /// User text, falling back to the field default.
    pub fn effective(&self, spec: &FieldSpec) -> String {
        let t = self.text(&spec.key);
        if t.is_empty() {
            spec.default_value.clone().unwrap_or_default()
        } else {
            t.to_string()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    #[error("required field '{0}' is empty")]
    MissingRequired(FieldKey),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderSegment {
    Text(String),
    Mention {
        field_key: FieldKey,
        query: String,
        display_name: String,
        required: bool,
    },
}

/// Illustration only. `@Name` here is NOT evidence of a real tag (spec §4A.5 invariant).
pub fn render_plain_text_preview(segments: &[RenderSegment]) -> String {
    segments
        .iter()
        .map(|s| match s {
            RenderSegment::Text(t) => t.clone(),
            RenderSegment::Mention { display_name, .. } => format!("@{display_name}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vals(pairs: &[(&str, &str)]) -> FieldValues {
        let mut v = FieldValues::new();
        for (k, t) in pairs {
            v.set(&FieldKey::new(k), *t);
        }
        v
    }

    #[test]
    fn placeholders_keep_fields_as_literal_brackets() {
        let t = Template::parse("Dạ [Tên:mention], mã [Mã].").unwrap();
        assert_eq!(
            t.render_placeholders(),
            vec![RenderSegment::Text("Dạ [Tên], mã [Mã].".into())]
        );
    }

    #[test]
    fn plain_placeholders_share_one_field() {
        let t = Template::parse("Chào [Tên], [Tên] ơi. [Nội dung]").unwrap();
        assert_eq!(t.fields.len(), 2);
        assert_eq!(t.fields[0].label, "Tên");
        let segs = t
            .render(&vals(&[("Tên", "Hiền"), ("Nội dung", "x")]))
            .unwrap();
        assert_eq!(
            segs,
            vec![RenderSegment::Text("Chào Hiền, Hiền ơi. x".into())]
        );
    }

    #[test]
    fn escapes_produce_literals() {
        let t = Template::parse(r"giữ \[Tên] và \\ ok").unwrap();
        assert!(t.fields.is_empty());
        assert_eq!(t.nodes, vec![Node::Text(r"giữ [Tên] và \ ok".into())]);
    }

    #[test]
    fn typed_syntax() {
        let t = Template::parse("[A:mention] [B:mention?] [C:multiline] [D:text]").unwrap();
        assert_eq!(t.fields[0].field_type, FieldType::Mention);
        assert_eq!(t.fields[0].mention_fallback, MentionFallback::RequireReal);
        assert_eq!(
            t.fields[1].mention_fallback,
            MentionFallback::AskTextOrMention
        );
        assert_eq!(t.fields[2].field_type, FieldType::Multiline);
        assert_eq!(t.fields[3].field_type, FieldType::Text);
    }

    #[test]
    fn errors_have_positions() {
        let e = Template::parse("ok\nabc [Tên").unwrap_err();
        assert_eq!((e.line, e.col, e.kind), (2, 5, TemplateErrorKind::Unclosed));
        assert!(matches!(
            Template::parse("[]").unwrap_err().kind,
            TemplateErrorKind::EmptyLabel
        ));
        assert!(matches!(
            Template::parse("[a:bogus]").unwrap_err().kind,
            TemplateErrorKind::UnknownType(_)
        ));
        assert!(matches!(
            Template::parse("[a [b]]").unwrap_err().kind,
            TemplateErrorKind::Nested
        ));
        let long = format!("[{}]", "x".repeat(65));
        assert_eq!(
            Template::parse(&long).unwrap_err().kind,
            TemplateErrorKind::LabelTooLong
        );
    }

    #[test]
    fn conflicting_explicit_types_error_but_plain_plus_typed_unifies() {
        let e = Template::parse("[T:mention] [T:multiline]").unwrap_err();
        assert!(matches!(e.kind, TemplateErrorKind::Conflict(_)));
        let e = Template::parse("[T:mention] [T:mention?]").unwrap_err();
        assert!(matches!(e.kind, TemplateErrorKind::Conflict(_)));
        let t = Template::parse("[T] và [T:mention]").unwrap();
        assert_eq!(t.fields.len(), 1);
        assert_eq!(t.fields[0].field_type, FieldType::Mention);
    }

    #[test]
    fn nfc_keys_unify_composed_and_decomposed() {
        let composed = "Tên";
        let decomposed = "Te\u{0302}n";
        let t = Template::parse(&format!("[{composed}] [{decomposed}]")).unwrap();
        assert_eq!(t.fields.len(), 1);
    }

    #[test]
    fn mention_renders_segments_in_order_and_never_as_text() {
        let t = Template::parse("Dạ [P:mention] và [Q:mention]: [N]").unwrap();
        let segs = t
            .render(&vals(&[("P", "Hiền"), ("Q", "Lan"), ("N", "ok")]))
            .unwrap();
        assert_eq!(segs.len(), 5);
        assert!(matches!(&segs[0], RenderSegment::Text(s) if s == "Dạ "));
        assert!(
            matches!(&segs[1], RenderSegment::Mention { query, required: true, .. } if query == "Hiền")
        );
        assert!(matches!(&segs[2], RenderSegment::Text(s) if s == " và "));
        assert!(matches!(&segs[3], RenderSegment::Mention { query, .. } if query == "Lan"));
        assert!(matches!(&segs[4], RenderSegment::Text(s) if s == ": ok"));
        // MN-07 / MN-02: no "@" text smuggled into Text segments.
        for s in &segs {
            if let RenderSegment::Text(t) = s {
                assert!(!t.contains('@'));
            }
        }
    }

    #[test]
    fn repeated_mention_gives_one_segment_per_occurrence() {
        let t = Template::parse("Dạ [Tên:mention], cập nhật [Tên:mention] sau").unwrap();
        let segs = t.render(&vals(&[("Tên", "Hiền")])).unwrap();
        let n = segs
            .iter()
            .filter(|s| matches!(s, RenderSegment::Mention { .. }))
            .count();
        assert_eq!(n, 2); // MN-08
    }

    #[test]
    fn mention_optional_plain_choice_gives_text_without_at() {
        let t = Template::parse("Dạ chị [Tên:mention?].").unwrap();
        let key = FieldKey::new("Tên");
        let mut v = vals(&[("Tên", "Hiền")]);
        v.set_choice(&key, MentionChoice::PlainText);
        let segs = t.render(&v).unwrap();
        assert_eq!(segs, vec![RenderSegment::Text("Dạ chị Hiền.".into())]); // MN-22
        v.set_choice(&key, MentionChoice::Tag);
        let segs = t.render(&v).unwrap();
        assert!(matches!(
            &segs[1],
            RenderSegment::Mention {
                required: false,
                ..
            }
        ));
    }

    #[test]
    fn override_turns_plain_field_into_mention_without_editing_body() {
        let body = "Hiện tại em đã nhận từ chị [Tên]. [Nội dung]";
        let mut t = Template::parse(body).unwrap();
        t.apply_overrides(&[FieldDefinition {
            key: "Tên".into(),
            field_type: FieldType::Mention,
            required: true,
            mention_fallback: MentionFallback::RequireReal,
            default_value: None,
        }]);
        let segs = t
            .render(&vals(&[("Tên", "Hiền"), ("Nội dung", "x")]))
            .unwrap();
        assert!(matches!(segs[1], RenderSegment::Mention { .. })); // MN-21
    }

    #[test]
    fn required_empty_blocks_render_and_defaults_fill() {
        let mut t = Template::parse("a [X] b").unwrap();
        assert!(matches!(
            t.render(&FieldValues::new()),
            Err(RenderError::MissingRequired(_))
        ));
        t.fields[0].default_value = Some("dflt".into());
        let segs = t.render(&FieldValues::new()).unwrap();
        assert_eq!(render_plain_text_preview(&segs), "a dflt b");
        t.fields[0].default_value = None;
        t.fields[0].required = false;
        assert_eq!(
            render_plain_text_preview(&t.render(&FieldValues::new()).unwrap()),
            "a  b"
        );
    }

    #[test]
    fn emoji_and_brackets_in_values_are_inert() {
        let t = Template::parse("[N]").unwrap();
        let segs = t.render(&vals(&[("N", "😀 [Tên] \\[x]")])).unwrap();
        assert_eq!(segs, vec![RenderSegment::Text("😀 [Tên] \\[x]".into())]); // values are not re-parsed
    }
}
