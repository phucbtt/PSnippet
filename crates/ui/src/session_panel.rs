//! Compact panel shown while a mention session waits for the user (spec §5A.1).
//! Deliberately small and not covering Zalo's suggestion dropdown (placed by the orchestrator).

use eframe::egui;

use crate::{ERR, OK, WARN};

pub struct SessionView {
    pub title: String,
    /// e.g. "Chờ chọn người 1/2".
    pub status: String,
    pub instruction: String,
    pub resume_hotkey: String,
    pub finished: bool,
    pub error: bool,
    /// Text the user can still copy (illustrative `@name`; mentions are not preserved).
    pub remaining: Option<String>,
}

pub enum SessionOutcome {
    None,
    Resume,
    CancelRemaining,
    CopyRemaining(String),
    Dismiss,
}

pub fn show_session_panel(ui: &mut egui::Ui, v: &SessionView) -> SessionOutcome {
    let mut out = SessionOutcome::None;
    ui.strong(&v.title);
    let color = if v.error {
        ERR
    } else if v.finished {
        OK
    } else {
        WARN
    };
    ui.colored_label(color, &v.status);
    ui.label(&v.instruction);
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if !v.finished {
            if ui
                .button(format!("Tiếp tục ({})", v.resume_hotkey))
                .clicked()
            {
                out = SessionOutcome::Resume;
            }
            if ui.button("Hủy bước còn lại").clicked() {
                out = SessionOutcome::CancelRemaining;
            }
        }
        if let Some(r) = &v.remaining
            && ui.button("Sao chép phần chưa chèn").clicked()
        {
            out = SessionOutcome::CopyRemaining(r.clone());
        }
        if v.finished && ui.button("Đóng").clicked() {
            out = SessionOutcome::Dismiss;
        }
    });
    out
}
