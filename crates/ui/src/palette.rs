//! Global snippet palette (spec §3.3). Enter selects a snippet inside the palette; it never sends chat.

use eframe::egui;
use zs_core::Snippet;

#[derive(Default)]
pub struct PaletteState {
    pub query: String,
    pub selected: usize,
    pub focus: bool,
}

impl PaletteState {
    pub fn fresh() -> Self {
        Self {
            focus: true,
            ..Default::default()
        }
    }
}

pub enum PaletteOutcome {
    None,
    /// Insert/fill this snippet into the captured target (form first if it has fields).
    Pick(Snippet),
    /// Copy the (static) snippet body.
    Copy(Snippet),
    Close,
}

fn matches(s: &Snippet, q: &str) -> bool {
    if q.is_empty() {
        return true;
    }
    let q = q.to_lowercase();
    s.code.to_lowercase().contains(&q)
        || s.title.to_lowercase().contains(&q)
        || s.body.to_lowercase().contains(&q)
        || s.tags.iter().any(|t| t.to_lowercase().contains(&q))
}

pub fn show_palette(ui: &mut egui::Ui, st: &mut PaletteState, all: &[Snippet]) -> PaletteOutcome {
    let mut out = PaletteOutcome::None;
    let hits: Vec<&Snippet> = {
        let mut v: Vec<&Snippet> = all
            .iter()
            .filter(|s| s.enabled && matches(s, &st.query))
            .collect();
        v.sort_by_key(|s| (!s.favorite, s.code.to_lowercase()));
        v.truncate(200);
        v
    };
    if !hits.is_empty() {
        st.selected = st.selected.min(hits.len() - 1);
    }

    let (down, up, enter, esc, ctrl_c) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::Enter) && !i.modifiers.command,
            i.key_pressed(egui::Key::Escape),
            i.modifiers.command && i.key_pressed(egui::Key::C),
        )
    });
    if down && !hits.is_empty() {
        st.selected = (st.selected + 1).min(hits.len() - 1);
    }
    if up {
        st.selected = st.selected.saturating_sub(1);
    }

    let r = ui.add(
        egui::TextEdit::singleline(&mut st.query)
            .hint_text("Tìm mẫu theo mã, tiêu đề, nội dung, tag…")
            .desired_width(f32::INFINITY),
    );
    if st.focus {
        r.request_focus();
        st.focus = false;
    }
    if r.changed() {
        st.selected = 0;
    }
    ui.separator();

    ui.columns(2, |cols| {
        egui::ScrollArea::vertical()
            .id_salt("pal_list")
            .show(&mut cols[0], |ui| {
                for (i, s) in hits.iter().enumerate() {
                    let label = format!(
                        "{}{}  —  {}",
                        if s.favorite { "★ " } else { "" },
                        s.code,
                        s.title
                    );
                    let resp = ui.selectable_label(i == st.selected, label);
                    if resp.clicked() {
                        st.selected = i;
                    }
                    if resp.double_clicked() {
                        out = PaletteOutcome::Pick((*s).clone());
                    }
                    if i == st.selected && (down || up) {
                        resp.scroll_to_me(None);
                    }
                }
                if hits.is_empty() {
                    ui.weak("Không có mẫu phù hợp.");
                }
            });
        egui::ScrollArea::vertical()
            .id_salt("pal_prev")
            .show(&mut cols[1], |ui| {
                if let Some(s) = hits.get(st.selected) {
                    ui.strong(&s.title);
                    if let Some(c) = &s.category {
                        ui.weak(c);
                    }
                    ui.add_space(4.0);
                    ui.label(&s.body);
                }
            });
    });

    if let Some(s) = hits.get(st.selected) {
        if enter {
            out = PaletteOutcome::Pick((*s).clone());
        }
        if ctrl_c {
            out = PaletteOutcome::Copy((*s).clone());
        }
    }
    if esc {
        out = PaletteOutcome::Close;
    }
    ui.weak("Enter: chọn mẫu · Ctrl+C: sao chép · Esc: đóng (Enter không gửi tin nhắn)");
    out
}
