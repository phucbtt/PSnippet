//! egui views for PSnippet: main window pages, variable form, palette, mention-session panel.
//! Views are pure functions of state; side effects are returned as `Cmd` / outcome values.

mod form;
mod main_view;
mod palette;
mod session_panel;

pub use eframe::egui;
pub use form::{FormOutcome, FormState, show_form};
pub use main_view::{Cmd, EngineStatus, MainView, Page};
pub use palette::{PaletteOutcome, PaletteState, show_palette};
pub use session_panel::{SessionOutcome, SessionView, show_session_panel};

/// Load a Vietnamese-capable system font (Segoe UI) if present; falls back to egui defaults.
/// The font file is read from the user's own Windows install, not bundled.
pub fn install_fonts(ctx: &egui::Context) {
    let win = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    for name in ["segoeui.ttf", "tahoma.ttf", "arial.ttf"] {
        let path = std::path::Path::new(&win).join("Fonts").join(name);
        if let Ok(bytes) = std::fs::read(&path) {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "system-ui".into(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes)),
            );
            // Put it first for text and as a fallback for monospace so Vietnamese never shows as boxes.
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "system-ui".into());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push("system-ui".into());
            ctx.set_fonts(fonts);
            return;
        }
    }
}

/// Brand/accent colour (primary buttons, selection, active nav item).
pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(38, 100, 200);

/// One consistent look: readable type scale, roomy spacing, rounded controls, accent selection.
/// Light/dark follow the OS theme (egui default); only metrics and accent are overridden.
pub fn apply_theme(ctx: &egui::Context) {
    use egui::{FontId, TextStyle};
    ctx.all_styles_mut(|st| {
        st.text_styles
            .insert(TextStyle::Heading, FontId::proportional(22.0));
        st.text_styles
            .insert(TextStyle::Body, FontId::proportional(14.5));
        st.text_styles
            .insert(TextStyle::Button, FontId::proportional(14.5));
        st.text_styles
            .insert(TextStyle::Small, FontId::proportional(12.0));
        st.text_styles
            .insert(TextStyle::Monospace, FontId::monospace(13.5));
        st.spacing.item_spacing = egui::vec2(8.0, 8.0);
        st.spacing.button_padding = egui::vec2(12.0, 6.0);
        st.spacing.interact_size.y = 28.0;
        st.spacing.window_margin = egui::Margin::same(14);
        let r = egui::CornerRadius::same(6);
        st.visuals.widgets.noninteractive.corner_radius = r;
        st.visuals.widgets.inactive.corner_radius = r;
        st.visuals.widgets.hovered.corner_radius = r;
        st.visuals.widgets.active.corner_radius = r;
        st.visuals.widgets.open.corner_radius = r;
        st.visuals.selection.bg_fill = ACCENT;
        st.visuals.selection.stroke.color = egui::Color32::WHITE;
        st.visuals.hyperlink_color = ACCENT;
    });
}

/// Filled accent button for the single primary action of a view.
pub fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(text)
                .color(egui::Color32::WHITE)
                .strong(),
        )
        .fill(ACCENT),
    )
}

/// Bordered, padded container used to group related content.
pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::group(ui.style())
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Small coloured status pill ("● Đang chạy").
pub fn status_pill(ui: &mut egui::Ui, color: egui::Color32, text: &str) {
    ui.label(
        egui::RichText::new(format!("●  {text}"))
            .color(color)
            .strong(),
    );
}

pub(crate) const WARN: egui::Color32 = egui::Color32::from_rgb(214, 140, 20);
pub(crate) const ERR: egui::Color32 = egui::Color32::from_rgb(210, 60, 60);
pub(crate) const OK: egui::Color32 = egui::Color32::from_rgb(60, 160, 90);

/// Chip text for an unconfirmed mention — visually and textually distinct from plain text (§5A.1).
pub fn mention_chip(name: &str) -> egui::RichText {
    egui::RichText::new(format!(" @{name} · chưa xác nhận "))
        .background_color(egui::Color32::from_rgb(40, 90, 160))
        .color(egui::Color32::WHITE)
}
