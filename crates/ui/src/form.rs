//! Variable form popup (spec §5.1, §5A.1): dynamic fields from the template AST, live preview,
//! mention fields with explicit "not yet confirmed" state. Never sends anything anywhere.

use eframe::egui;
use zs_core::{
    FieldType, FieldValues, MentionChoice, MentionFallback, RenderSegment, Snippet, Template,
    render_plain_text_preview,
};

use crate::{ERR, WARN, mention_chip};

pub struct FormState {
    pub snippet: Snippet,
    pub template: Template,
    pub values: FieldValues,
    /// Why Insert is unavailable (e.g. target changed ⇒ copy-only), shown as a banner.
    pub banner: Option<String>,
    pub insert_enabled: bool,
    /// The chat cannot be identified by the app (strict mode): Insert needs an explicit user acknowledgement.
    pub needs_chat_ack: bool,
    pub chat_ack: bool,
    pub show_missing: bool,
    pub focus_first: bool,
}

impl FormState {
    pub fn new(snippet: Snippet, template: Template) -> Self {
        let mut values = FieldValues::new();
        for f in &template.fields {
            if let Some(d) = &f.default_value {
                values.set(&f.key, d.clone());
            }
        }
        Self {
            snippet,
            template,
            values,
            banner: None,
            insert_enabled: true,
            needs_chat_ack: false,
            chat_ack: false,
            show_missing: false,
            focus_first: true,
        }
    }
}

pub enum FormOutcome {
    None,
    /// Confirmed, valid. The orchestrator re-validates the target before touching it.
    /// Confirmed segments + whether the user acknowledged the chat is the right one.
    Insert(Vec<RenderSegment>, bool),
    Copy(String),
    Cancel,
}

pub fn show_form(ui: &mut egui::Ui, st: &mut FormState, live_preview: bool) -> FormOutcome {
    let mut outcome = FormOutcome::None;

    ui.horizontal(|ui| {
        ui.heading(&st.snippet.title);
        ui.weak(format!("::{}::", st.snippet.code));
    });
    if let Some(b) = &st.banner {
        ui.colored_label(WARN, b);
    }
    ui.separator();

    let missing = st.template.missing_required(&st.values);
    let fields = st.template.fields.clone();
    let mut first = true;
    egui::Grid::new("form_fields")
        .num_columns(2)
        .spacing([10.0, 8.0])
        .show(ui, |ui| {
            for f in &fields {
                let is_missing = st.show_missing && missing.contains(&f.key);
                let label = match f.field_type {
                    FieldType::Mention => format!(
                        "{} (người cần tag){}",
                        f.label,
                        if f.required { " *" } else { "" }
                    ),
                    _ => format!("{}{}", f.label, if f.required { " *" } else { "" }),
                };
                ui.label(label);
                ui.vertical(|ui| {
                    let mut text = st.values.text(&f.key).to_string();
                    let resp = match f.field_type {
                        FieldType::Multiline => ui.add(
                            egui::TextEdit::multiline(&mut text)
                                .desired_rows(3)
                                .desired_width(f32::INFINITY),
                        ),
                        _ => ui.add(
                            egui::TextEdit::singleline(&mut text).desired_width(f32::INFINITY),
                        ),
                    };
                    if first && st.focus_first {
                        resp.request_focus();
                        st.focus_first = false;
                    }
                    first = false;
                    if resp.changed() {
                        st.values.set(&f.key, text);
                    }
                    if f.field_type == FieldType::Mention {
                        if f.mention_fallback == MentionFallback::AskTextOrMention {
                            let mut c = st.values.choice(&f.key);
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut c, MentionChoice::Tag, "Tag trong Zalo");
                                ui.radio_value(
                                    &mut c,
                                    MentionChoice::PlainText,
                                    "Chèn tên dạng văn bản",
                                );
                            });
                            st.values.set_choice(&f.key, c);
                        }
                        ui.weak("Tag chỉ hoàn tất sau khi chọn người trong cửa sổ Zalo.");
                    }
                    if is_missing {
                        ui.colored_label(ERR, "Trường bắt buộc");
                    }
                });
                ui.end_row();
            }
        });

    ui.add_space(6.0);
    let rendered = st.template.render(&st.values);
    if live_preview {
        ui.separator();
        ui.label("Xem trước");
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            egui::ScrollArea::vertical()
                .id_salt("form_preview")
                .max_height(180.0)
                .show(ui, |ui| match &rendered {
                    Ok(segs) => {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            for s in segs {
                                match s {
                                    RenderSegment::Text(t) => {
                                        ui.label(t);
                                    }
                                    RenderSegment::Mention { display_name, .. } => {
                                        ui.label(mention_chip(display_name));
                                    }
                                }
                            }
                        });
                    }
                    Err(_) => {
                        ui.weak("Điền đủ các trường bắt buộc để xem bản nháp.");
                    }
                });
        });
    }

    ui.add_space(6.0);
    let confirm_kb = ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if st.insert_enabled && st.needs_chat_ack {
        ui.checkbox(
            &mut st.chat_ack,
            "Tôi đã kiểm tra: đây đúng cuộc trò chuyện cần chèn (app không tự xác minh được)",
        );
    }
    let can_insert = st.insert_enabled && (!st.needs_chat_ack || st.chat_ack);
    ui.horizontal(|ui| {
        let can = rendered.is_ok();
        let insert = ui.add_enabled(
            can_insert,
            egui::Button::new(
                egui::RichText::new("Chèn vào ứng dụng (Ctrl+Enter)")
                    .color(egui::Color32::WHITE)
                    .strong(),
            )
            .fill(crate::ACCENT),
        );
        if (insert.clicked() || (confirm_kb && can_insert))
            && !matches!(outcome, FormOutcome::Cancel)
        {
            match &rendered {
                Ok(segs) if can => outcome = FormOutcome::Insert(segs.clone(), st.chat_ack),
                _ => st.show_missing = true,
            }
        }
        if ui.button("Sao chép").clicked() {
            match &rendered {
                Ok(segs) => outcome = FormOutcome::Copy(render_plain_text_preview(segs)),
                Err(_) => st.show_missing = true,
            }
        }
        if ui.button("Hủy (Esc)").clicked() || esc {
            outcome = FormOutcome::Cancel;
        }
    });
    if st
        .template
        .fields
        .iter()
        .any(|f| f.field_type == FieldType::Mention)
    {
        ui.weak("Sao chép chỉ chép văn bản minh họa: @Tên KHÔNG phải là tag thật.");
    }
    outcome
}
