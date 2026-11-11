//! Main management window (spec §5.1): Dashboard, Snippets, Categories, Import/Export, App Rules,
//! Settings, Diagnostics.

use eframe::egui;
use uuid::Uuid;
use zs_core::{
    Config, DelimKey, FieldDefinition, FieldType, MentionFallback, MentionModeCfg, Snippet,
    find_duplicate_code, is_valid_code,
};
use zs_storage::{ConflictPolicy, Store};

use crate::{ACCENT, ERR, OK, WARN, card, primary_button, status_pill};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Dashboard,
    Snippets,
    Categories,
    ImportExport,
    AppRules,
    Settings,
    Diagnostics,
    About,
}

const PAGES: &[(Page, &str)] = &[
    (Page::Dashboard, "Tổng quan"),
    (Page::Snippets, "Mẫu gõ tắt"),
    (Page::Categories, "Nhóm"),
    (Page::ImportExport, "Nhập / Xuất"),
    (Page::AppRules, "Ứng dụng"),
    (Page::Settings, "Cài đặt"),
    (Page::Diagnostics, "Chẩn đoán"),
    (Page::About, "Giới thiệu"),
];

/// Read-only engine state pushed in by the orchestrator each frame.
#[derive(Default, Clone)]
pub struct EngineStatus {
    pub paused: bool,
    pub hook_running: bool,
    pub overflow: u64,
    /// Times the watchdog had to reinstall the low-level hooks.
    pub hook_reinstalls: u64,
    /// Keys seen while an IME was composing (typed-text window dropped).
    pub ime_keys: u64,
    pub session_status: String,
    pub log: Vec<String>,
    pub hotkey_problems: Vec<String>,
    pub foreground_exe: Option<String>,
    pub mention_effective_mode: String,
}

/// Side effects the orchestrator must perform.
#[derive(Debug)]
pub enum Cmd {
    ConfigChanged,
    SnippetsChanged,
    TogglePause,
    /// Capture the foreground app's exe after a short delay and add it to the allowlist.
    CaptureForegroundApp,
    OpenPalette,
    /// Open the form for this snippet in test mode (copy only).
    TestSnippet(Snippet),
    RestoredFromBackup,
}

struct Edit {
    s: Snippet,
    is_new: bool,
    tags: String,
    error: Option<String>,
    confirm_delete: bool,
}

impl Edit {
    fn from(s: Snippet, is_new: bool) -> Self {
        let tags = s.tags.join(", ");
        Self {
            s,
            is_new,
            tags,
            error: None,
            confirm_delete: false,
        }
    }
}

pub struct MainView {
    pub page: Page,
    search: String,
    cache: Vec<Snippet>,
    dirty: bool,
    edit: Option<Edit>,
    new_category: String,
    incoming: Vec<Snippet>,
    incoming_src: String,
    policy: ConflictPolicy,
    ie_msg: Option<(bool, String)>,
    new_app: String,
    app_msg: Option<String>,
}

impl Default for MainView {
    fn default() -> Self {
        Self {
            page: Page::Dashboard,
            search: String::new(),
            cache: Vec::new(),
            dirty: true,
            edit: None,
            new_category: String::new(),
            incoming: Vec::new(),
            incoming_src: String::new(),
            policy: ConflictPolicy::Skip,
            ie_msg: None,
            new_app: String::new(),
            app_msg: None,
        }
    }
}

impl MainView {
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    pub fn set_app_message(&mut self, m: impl Into<String>) {
        self.app_msg = Some(m.into());
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        cfg: &mut Config,
        st: &EngineStatus,
    ) -> Vec<Cmd> {
        let mut cmds = Vec::new();
        if self.dirty {
            self.cache = store.all().unwrap_or_default();
            self.dirty = false;
        }
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(150.0);
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("PSnippet")
                        .size(20.0)
                        .strong()
                        .color(ACCENT),
                );
                let (c, t) = if !st.hook_running {
                    (ERR, "Chưa chạy")
                } else if st.paused {
                    (WARN, "Đã tắt gõ tắt")
                } else {
                    (OK, "Đang bật")
                };
                status_pill(ui, c, t);
                ui.add_space(6.0);
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    for (p, label) in PAGES {
                        let text = egui::RichText::new(*label).size(15.0);
                        if ui.selectable_label(self.page == *p, text).clicked() {
                            self.page = *p;
                        }
                    }
                });
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_min_width(ui.available_width());
                match self.page {
                    Page::Dashboard => self.page_dashboard(ui, cfg, st, &mut cmds),
                    Page::Snippets => self.page_snippets(ui, store, &mut cmds),
                    Page::Categories => self.page_categories(ui, store),
                    Page::ImportExport => self.page_import_export(ui, store, &mut cmds),
                    Page::AppRules => self.page_app_rules(ui, cfg, st, &mut cmds),
                    Page::Settings => self.page_settings(ui, cfg, &mut cmds),
                    Page::Diagnostics => self.page_diagnostics(ui, cfg, st),
                    Page::About => self.page_about(ui),
                }
            });
        });
        cmds
    }

    // ---- Dashboard ---------------------------------------------------------------------------

    fn page_dashboard(
        &mut self,
        ui: &mut egui::Ui,
        cfg: &Config,
        st: &EngineStatus,
        cmds: &mut Vec<Cmd>,
    ) {
        ui.heading("Tổng quan");
        ui.weak(format!(
            "{} mẫu · {} đang bật",
            self.cache.len(),
            self.cache.iter().filter(|s| s.enabled).count()
        ));
        ui.add_space(4.0);

        card(ui, |ui| {
            ui.horizontal(|ui| {
                let (t, c) = if !st.hook_running {
                    ("Hook bàn phím không chạy", ERR)
                } else if st.paused {
                    ("Gõ tắt đang TẮT", WARN)
                } else {
                    ("Gõ tắt đang BẬT", OK)
                };
                status_pill(ui, c, t);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Mở palette").clicked() {
                        cmds.push(Cmd::OpenPalette);
                    }
                    let label = if st.paused {
                        "Bật lại"
                    } else {
                        "Tạm tắt"
                    };
                    let clicked = if st.paused {
                        primary_button(ui, label).clicked()
                    } else {
                        ui.button(label).clicked()
                    };
                    if clicked {
                        cmds.push(Cmd::TogglePause);
                    }
                });
            });
        });

        if cfg.allowed_apps.is_empty() {
            card(ui, |ui| {
                ui.colored_label(WARN, "Chưa chọn ứng dụng nào");
                ui.label("Gõ tắt đang vô hiệu ở mọi nơi. Mở mục Ứng dụng để thêm Zalo (hoặc Notepad để thử).");
                if ui.button("Đi tới Ứng dụng").clicked() {
                    self.page = Page::AppRules;
                }
            });
        }
        if !cfg.strict_focus {
            card(ui, |ui| {
                ui.colored_label(WARN, "Chế độ tương thích đang bật");
                ui.label("Không xác minh được ô nhập/cuộc trò chuyện trước khi chèn. Hãy kiểm tra bản nháp trước khi gửi.");
            });
        }

        card(ui, |ui| {
            ui.strong("Cách dùng");
            ui.label("1. Thêm ứng dụng (Zalo…) vào mục Ứng dụng.");
            ui.label("2. Trong ứng dụng đó, gõ mã (ví dụ ztn) rồi nhấn Space.");
            ui.label("3. Nội dung được thay ngay tại chỗ; chỗ [Tên] cần bạn sửa tay.");
            ui.weak(
                "Enter không bao giờ là phím kích hoạt và PSnippet không bao giờ tự gửi tin nhắn.",
            );
        });

        card(ui, |ui| {
            ui.strong("Phím tắt");
            egui::Grid::new("dash_keys")
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Bật/tắt gõ tắt");
                    ui.monospace(&cfg.toggle_hotkey);
                    ui.end_row();
                    ui.label("Mở palette");
                    ui.monospace(&cfg.palette_hotkey);
                    ui.end_row();
                });
        });
        ui.weak("Tag (@) thật chỉ hoàn tất khi bạn chọn thành viên trong gợi ý của Zalo; chưa kiểm chứng trên Zalo PC thực tế.");
    }

    // ---- About -------------------------------------------------------------------------------

    fn page_about(&mut self, ui: &mut egui::Ui) {
        ui.heading("Giới thiệu");
        card(ui, |ui| {
            ui.label(
                egui::RichText::new("PSnippet")
                    .size(24.0)
                    .strong()
                    .color(ACCENT),
            );
            ui.label(format!("Phiên bản {}", env!("CARGO_PKG_VERSION")));
            ui.add_space(4.0);
            ui.label("Trình gõ tắt offline, mã nguồn mở cho Windows: gõ mã rồi nhấn Space để thay bằng mẫu nhiều dòng.");
            ui.weak("PSnippet không bao giờ tự gửi tin nhắn, không đọc nội dung cuộc trò chuyện, không dùng mạng.");
        });
        card(ui, |ui| {
            ui.hyperlink_to(
                "Mã nguồn trên GitHub",
                "https://github.com/phucbtt/PSnippet",
            );
            ui.hyperlink_to(
                "Bản phát hành (Releases)",
                "https://github.com/phucbtt/PSnippet/releases",
            );
            ui.hyperlink_to(
                "Báo lỗi / góp ý",
                "https://github.com/phucbtt/PSnippet/issues",
            );
            ui.add_space(4.0);
            ui.weak("Giấy phép MIT. Dự án độc lập, không liên kết với hay được xác nhận bởi Zalo / VNG.");
        });
    }

    // ---- Snippets ----------------------------------------------------------------------------

    fn page_snippets(&mut self, ui: &mut egui::Ui, store: &mut Store, cmds: &mut Vec<Cmd>) {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Tìm theo mã, tiêu đề, nội dung, tag")
                    .desired_width(260.0),
            );
            if primary_button(ui, "+ Mẫu mới").clicked() {
                self.edit = Some(Edit::from(Snippet::new("", "", ""), true));
            }
        });
        ui.separator();

        let needle = self.search.trim().to_lowercase();
        let rows: Vec<(Uuid, String, bool)> = self
            .cache
            .iter()
            .filter(|s| {
                needle.is_empty()
                    || s.code.to_lowercase().contains(&needle)
                    || s.title.to_lowercase().contains(&needle)
                    || s.body.to_lowercase().contains(&needle)
                    || s.tags.iter().any(|t| t.to_lowercase().contains(&needle))
            })
            .map(|s| {
                (
                    s.id,
                    format!(
                        "{}{}  {}  [{}]",
                        if s.favorite { "★ " } else { "" },
                        s.code,
                        s.title,
                        s.category.clone().unwrap_or_default()
                    ),
                    s.enabled,
                )
            })
            .collect();
        let selected = self.edit.as_ref().map(|e| e.s.id);
        let mut to_open: Option<Uuid> = None;

        ui.columns(2, |cols| {
            egui::ScrollArea::vertical()
                .id_salt("snip_list")
                .show(&mut cols[0], |ui| {
                    for (id, label, enabled) in &rows {
                        let text = if *enabled {
                            egui::RichText::new(label)
                        } else {
                            egui::RichText::new(label).weak()
                        };
                        if ui.selectable_label(selected == Some(*id), text).clicked() {
                            to_open = Some(*id);
                        }
                    }
                    if rows.is_empty() {
                        ui.add_space(12.0);
                        ui.weak(if needle.is_empty() {
                            "Chưa có mẫu nào. Bấm \"+ Mẫu mới\" để tạo."
                        } else {
                            "Không có mẫu khớp từ khóa tìm kiếm."
                        });
                    }
                });
            self.snippet_editor(&mut cols[1], store, cmds);
        });
        if let Some(id) = to_open
            && let Some(s) = self.cache.iter().find(|s| s.id == id)
        {
            self.edit = Some(Edit::from(s.clone(), false));
        }
    }

    fn snippet_editor(&mut self, ui: &mut egui::Ui, store: &mut Store, cmds: &mut Vec<Cmd>) {
        let Some(edit) = self.edit.as_mut() else {
            ui.add_space(12.0);
            ui.weak("Chọn một mẫu ở danh sách bên trái hoặc tạo mẫu mới.");
            return;
        };
        let mut action: Option<&str> = None;
        egui::ScrollArea::vertical().id_salt("snip_editor").show(ui, |ui| {
            egui::Grid::new("edit_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Mã (code)");
                ui.add(egui::TextEdit::singleline(&mut edit.s.code).desired_width(160.0));
                ui.end_row();
                ui.label("Tiêu đề");
                ui.add(egui::TextEdit::singleline(&mut edit.s.title).desired_width(f32::INFINITY));
                ui.end_row();
                ui.label("Nhóm");
                let mut cat = edit.s.category.clone().unwrap_or_default();
                ui.add(egui::TextEdit::singleline(&mut cat).desired_width(200.0));
                edit.s.category = (!cat.trim().is_empty()).then(|| cat.trim().to_string());
                ui.end_row();
                ui.label("Tag");
                ui.add(egui::TextEdit::singleline(&mut edit.tags).hint_text("phân tách bằng dấu phẩy").desired_width(f32::INFINITY));
                ui.end_row();
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut edit.s.enabled, "Bật");
                ui.checkbox(&mut edit.s.favorite, "Yêu thích");
            });
            ui.label("Nội dung  ([Biến], [Tên:mention], [Tên:mention?], [Nội dung:multiline], \\[ để chèn dấu [ )");
            ui.add(
                egui::TextEdit::multiline(&mut edit.s.body).desired_rows(9).desired_width(f32::INFINITY).code_editor(),
            );

            // Field table: type / required / default for each placeholder.
            match zs_core::Template::parse(&edit.s.body) {
                Err(e) => {
                    ui.colored_label(ERR, format!("Dòng {}, cột {}: {}", e.line, e.col, e.kind));
                }
                Ok(_) => {
                    let tpl = edit.s.template();
                    if let Ok(tpl) = tpl
                        && !tpl.fields.is_empty() {
                            ui.separator();
                            ui.strong("Bảng biến");
                            let mut changed: Vec<FieldDefinition> = Vec::new();
                            egui::Grid::new("field_table").striped(true).spacing([8.0, 4.0]).show(ui, |ui| {
                                ui.weak("Key");
                                ui.weak("Kiểu");
                                ui.weak("Bắt buộc");
                                ui.weak("Mặc định");
                                ui.weak("Tag/Text");
                                ui.end_row();
                                for f in &tpl.fields {
                                    let mut d = FieldDefinition::from_spec(f);
                                    let before = d.clone();
                                    ui.label(&f.label);
                                    ui.add_enabled_ui(!f.explicit_type, |ui| {
                                        egui::ComboBox::from_id_salt(("ft", &f.key)).selected_text(type_name(d.field_type)).show_ui(ui, |ui| {
                                            ui.selectable_value(&mut d.field_type, FieldType::Text, "text");
                                            ui.selectable_value(&mut d.field_type, FieldType::Multiline, "multiline");
                                            ui.selectable_value(&mut d.field_type, FieldType::Mention, "mention");
                                        });
                                    });
                                    ui.checkbox(&mut d.required, "");
                                    let mut dv = d.default_value.clone().unwrap_or_default();
                                    ui.add(egui::TextEdit::singleline(&mut dv).desired_width(110.0));
                                    d.default_value = (!dv.is_empty()).then_some(dv);
                                    if d.field_type == FieldType::Mention && !f.explicit_type {
                                        let mut ask = d.mention_fallback == MentionFallback::AskTextOrMention;
                                        ui.checkbox(&mut ask, "cho chọn text");
                                        d.mention_fallback =
                                            if ask { MentionFallback::AskTextOrMention } else { MentionFallback::RequireReal };
                                    } else if f.explicit_type && f.field_type == FieldType::Mention {
                                        ui.weak("khai báo trong nội dung");
                                    } else {
                                        ui.label("");
                                    }
                                    ui.end_row();
                                    if d != before {
                                        changed.push(d);
                                    }
                                }
                            });
                            for d in changed {
                                edit.s.fields.retain(|x| x.key != d.key);
                                edit.s.fields.push(d);
                            }
                        }
                }
            }

            if let Some(e) = &edit.error {
                ui.colored_label(ERR, e);
            }
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                if primary_button(ui, "Lưu").clicked() {
                    action = Some("save");
                }
                if ui.button("Thử mẫu").clicked() {
                    action = Some("test");
                }
                if !edit.is_new && ui.button("Nhân bản").clicked() {
                    action = Some("dup");
                }
                if !edit.is_new {
                    ui.checkbox(&mut edit.confirm_delete, "xác nhận xóa");
                    if ui.add_enabled(edit.confirm_delete, egui::Button::new("Xóa")).clicked() {
                        action = Some("delete");
                    }
                }
            });
        });

        match action {
            Some("save") => {
                let r = save_edit(edit, store, &self.cache);
                match r {
                    Ok(()) => {
                        edit.error = None;
                        edit.is_new = false;
                        self.dirty = true;
                        cmds.push(Cmd::SnippetsChanged);
                    }
                    Err(e) => edit.error = Some(e),
                }
            }
            Some("test") => {
                let mut s = edit.s.clone();
                s.tags = split_tags(&edit.tags);
                cmds.push(Cmd::TestSnippet(s));
            }
            Some("dup") => {
                let mut s = edit.s.clone();
                s.id = Uuid::new_v4();
                s.code = format!("{}_copy", s.code.chars().take(26).collect::<String>());
                self.edit = Some(Edit::from(s, true));
            }
            Some("delete") => {
                let id = edit.s.id;
                if store.delete(id).is_ok() {
                    self.edit = None;
                    self.dirty = true;
                    cmds.push(Cmd::SnippetsChanged);
                }
            }
            _ => {}
        }
    }

    // ---- Categories --------------------------------------------------------------------------

    fn page_categories(&mut self, ui: &mut egui::Ui, store: &mut Store) {
        ui.heading("Nhóm");
        for c in store.categories().unwrap_or_default() {
            let n = self
                .cache
                .iter()
                .filter(|s| s.category.as_deref() == Some(c.as_str()))
                .count();
            ui.label(format!("{c}  ({n})"));
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.new_category);
            if ui.button("Thêm nhóm").clicked() && !self.new_category.trim().is_empty() {
                let _ = store.ensure_category(self.new_category.trim());
                self.new_category.clear();
            }
        });
    }

    // ---- Import / Export ---------------------------------------------------------------------

    fn page_import_export(&mut self, ui: &mut egui::Ui, store: &mut Store, cmds: &mut Vec<Cmd>) {
        ui.heading("Nhập");
        ui.horizontal(|ui| {
            if ui.button("Chọn tệp (.zsnip / .json / .ahk.txt)…").clicked()
                && let Some(p) = rfd::FileDialog::new()
                    .add_filter("Mẫu", &["zsnip", "json", "txt", "ahk"])
                    .pick_file()
            {
                match std::fs::read(&p).map_err(|e| e.to_string()).and_then(|b| {
                    String::from_utf8(b).map_err(|_| "Tệp không phải UTF-8".to_string())
                }) {
                    Err(e) => self.ie_msg = Some((false, e)),
                    Ok(text) => {
                        let t = text.trim_start_matches('\u{feff}').trim_start();
                        let parsed = if t.starts_with('{') {
                            zs_parser::import_json(&text).map_err(|e| e.to_string())
                        } else {
                            zs_parser::parse_legacy(&text)
                                .map(|v| v.into_iter().map(|l| l.into_snippet()).collect())
                                .map_err(|e| e.to_string())
                        };
                        match parsed {
                            Ok(v) => {
                                self.ie_msg = Some((
                                    true,
                                    format!("Đọc được {} mẫu từ {}", v.len(), p.display()),
                                ));
                                self.incoming = v;
                                self.incoming_src = p.display().to_string();
                            }
                            Err(e) => {
                                self.incoming.clear();
                                self.ie_msg = Some((false, e));
                            }
                        }
                    }
                }
            }
        });
        if !self.incoming.is_empty() {
            ui.label(format!("Xem trước ({}):", self.incoming_src));
            egui::ScrollArea::vertical()
                .id_salt("ie_prev")
                .max_height(180.0)
                .show(ui, |ui| {
                    let dup_in_file =
                        find_duplicate_code(self.incoming.iter().map(|s| s.code.as_str()));
                    for s in &self.incoming {
                        let exists = store.get_by_code(&s.code).ok().flatten().is_some();
                        ui.horizontal(|ui| {
                            ui.label(format!("{}  {}", s.code, s.title));
                            if exists {
                                ui.colored_label(WARN, "trùng mã");
                            } else {
                                ui.colored_label(OK, "mới");
                            }
                        });
                    }
                    if let Some(d) = dup_in_file {
                        ui.colored_label(ERR, format!("Tệp có mã trùng: {}", d.0));
                    }
                });
            ui.horizontal(|ui| {
                ui.label("Khi trùng mã:");
                ui.radio_value(&mut self.policy, ConflictPolicy::Skip, "Bỏ qua");
                ui.radio_value(&mut self.policy, ConflictPolicy::Replace, "Thay thế");
                ui.radio_value(&mut self.policy, ConflictPolicy::Rename, "Đổi tên");
            });
            if ui.button("Áp dụng nhập").clicked() {
                match store.import(std::mem::take(&mut self.incoming), self.policy) {
                    Ok(r) => {
                        self.ie_msg = Some((
                            true,
                            format!(
                                "Thêm {} · bỏ qua {} · thay {} · đổi tên {}",
                                r.added.len(),
                                r.skipped.len(),
                                r.replaced.len(),
                                r.renamed.len()
                            ),
                        ));
                        self.dirty = true;
                        cmds.push(Cmd::SnippetsChanged);
                    }
                    Err(e) => self.ie_msg = Some((false, e.to_string())),
                }
            }
        }
        ui.separator();
        ui.heading("Xuất / Sao lưu");
        ui.horizontal_wrapped(|ui| {
            if ui.button("Xuất .zsnip (JSON)").clicked()
                && let Some(p) = rfd::FileDialog::new().set_file_name("snippets.zsnip").save_file() {
                    let r = store
                        .all()
                        .map_err(|e| e.to_string())
                        .and_then(|a| zs_parser::export_json(&a, &iso_now()).map_err(|e| e.to_string()))
                        .and_then(|j| std::fs::write(&p, j).map_err(|e| e.to_string()));
                    self.ie_msg = Some(match r {
                        Ok(()) => (true, format!("Đã xuất {}", p.display())),
                        Err(e) => (false, e),
                    });
                }
            if ui.button("Xuất legacy .ahk.txt").clicked()
                && let Some(p) = rfd::FileDialog::new().set_file_name("snippets.ahk.txt").save_file() {
                    let r = store.all().map_err(|e| e.to_string()).and_then(|a| {
                        std::fs::write(&p, zs_parser::export_legacy(&a)).map_err(|e| e.to_string())
                    });
                    self.ie_msg = Some(match r {
                        Ok(()) => (true, format!("Đã xuất {} (metadata trường nâng cao không được lưu trong định dạng legacy)", p.display())),
                        Err(e) => (false, e),
                    });
                }
            if ui.button("Sao lưu CSDL…").clicked()
                && let Some(p) = rfd::FileDialog::new().set_file_name("psnippet-backup.db").save_file() {
                    self.ie_msg = Some(match store.backup_to(&p) {
                        Ok(()) => (true, format!("Đã sao lưu {}", p.display())),
                        Err(e) => (false, e.to_string()),
                    });
                }
            if ui.button("Khôi phục từ sao lưu…").clicked()
                && let Some(p) = rfd::FileDialog::new().add_filter("SQLite", &["db"]).pick_file() {
                    self.ie_msg = Some(match store.restore_from(&p) {
                        Ok(()) => {
                            self.dirty = true;
                            cmds.push(Cmd::RestoredFromBackup);
                            (true, "Đã khôi phục".to_string())
                        }
                        Err(e) => (false, e.to_string()),
                    });
                }
        });
        if let Some((ok, m)) = &self.ie_msg {
            ui.colored_label(if *ok { OK } else { ERR }, m);
        }
    }

    // ---- App rules ---------------------------------------------------------------------------

    fn page_app_rules(
        &mut self,
        ui: &mut egui::Ui,
        cfg: &mut Config,
        st: &EngineStatus,
        cmds: &mut Vec<Cmd>,
    ) {
        ui.heading("Ứng dụng được phép");
        ui.label("Gõ tắt chỉ hoạt động khi cửa sổ đang dùng thuộc một ứng dụng dưới đây. Không có mục nào ⇒ vô hiệu ở mọi nơi.");
        let mut remove: Option<usize> = None;
        for (i, a) in cfg.allowed_apps.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(a);
                if ui.small_button("Xóa").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            cfg.allowed_apps.remove(i);
            cmds.push(Cmd::ConfigChanged);
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.new_app)
                    .hint_text(r"Zalo.exe hoặc C:\đường\dẫn\app.exe")
                    .desired_width(300.0),
            );
            if ui.button("Thêm").clicked() && !self.new_app.trim().is_empty() {
                cfg.allowed_apps.push(self.new_app.trim().to_string());
                self.new_app.clear();
                cmds.push(Cmd::ConfigChanged);
            }
        });
        if ui
            .button("Thêm ứng dụng đang dùng (chọn cửa sổ trong 3 giây)")
            .clicked()
        {
            self.app_msg = Some("Hãy bấm vào cửa sổ ứng dụng cần thêm…".into());
            cmds.push(Cmd::CaptureForegroundApp);
        }
        if let Some(m) = &self.app_msg {
            ui.colored_label(WARN, m);
        }
        if let Some(fg) = &st.foreground_exe {
            ui.weak(format!("Cửa sổ trước đó: {fg}"));
        }
        ui.weak(
            "Tên tiến trình Zalo không được mã hóa cứng: hãy chọn bằng nút trên khi đang mở Zalo.",
        );
    }

    // ---- Settings ----------------------------------------------------------------------------

    fn page_settings(&mut self, ui: &mut egui::Ui, cfg: &mut Config, cmds: &mut Vec<Cmd>) {
        let before = cfg.clone();
        egui::ScrollArea::vertical().id_salt("settings").show(ui, |ui| {
            ui.heading("Gõ tắt");
            ui.checkbox(&mut cfg.hotstring_enabled, "Bật gõ tắt");
            let mut space = cfg.trigger_delimiters.contains(&DelimKey::Space);
            let mut tab = cfg.trigger_delimiters.contains(&DelimKey::Tab);
            ui.horizontal(|ui| {
                ui.label("Phím kích hoạt:");
                ui.checkbox(&mut space, "Space");
                ui.checkbox(&mut tab, "Tab");
                ui.weak("(Enter không bao giờ được dùng)");
            });
            cfg.trigger_delimiters.clear();
            if space || !tab {
                cfg.trigger_delimiters.push(DelimKey::Space);
            }
            if tab {
                cfg.trigger_delimiters.push(DelimKey::Tab);
            }
            ui.checkbox(&mut cfg.case_sensitive, "Phân biệt hoa/thường");
            ui.checkbox(&mut cfg.trigger_literal_mode, "Chế độ literal: gõ đúng ::mã:: để kích hoạt");
            ui.checkbox(&mut cfg.require_word_boundary, "Chỉ kích hoạt ở ranh giới từ");
            ui.checkbox(&mut cfg.preserve_delimiter, "Giữ ký tự phân cách (Space) sau nội dung");
            ui.separator();
            ui.heading("An toàn");
            ui.checkbox(&mut cfg.strict_focus, "Strict: chỉ chèn khi xác minh được ô nhập (fail-closed)");
            if !cfg.strict_focus {
                ui.colored_label(WARN, "Chế độ tương thích: có thể chèn khi không xác minh được ô nhập hoặc cuộc trò chuyện. Chỉ bật sau khi đã thử trên Zalo của bạn.");
            }
            ui.horizontal(|ui| {
                ui.label("Khôi phục clipboard:");
                egui::ComboBox::from_id_salt("clip_restore").selected_text(cfg.clipboard_restore.clone()).show_ui(ui, |ui| {
                    ui.selectable_value(&mut cfg.clipboard_restore, "best_effort".to_string(), "best_effort");
                    ui.selectable_value(&mut cfg.clipboard_restore, "never".to_string(), "never");
                });
            });
            ui.checkbox(
                &mut cfg.instant_expand,
                "Gõ tắt kiểu UniKey: mã + Space thay thế ngay tại chỗ (biến giữ nguyên dạng [Tên]); tắt để mở form điền biến",
            );
            ui.checkbox(&mut cfg.live_preview, "Xem trước trực tiếp trong form");
            ui.checkbox(&mut cfg.remember_field_values, "Ghi nhớ giá trị trường (mặc định tắt để tránh lưu thông tin khách hàng; không lưu tên người cần tag)");
            ui.separator();
            ui.heading("Phím tắt toàn cục");
            hotkey_row(ui, "Mở palette", &mut cfg.palette_hotkey);
            hotkey_row(ui, "Tiếp tục sau khi chọn tag", &mut cfg.mention_resume_hotkey);
            hotkey_row(ui, "Hủy phiên chèn", &mut cfg.mention_cancel_hotkey);
            hotkey_row(ui, "Bật/tắt gõ tắt", &mut cfg.toggle_hotkey);
            ui.separator();
            ui.heading("Mention");
            ui.horizontal(|ui| {
                ui.label("Chế độ:");
                egui::ComboBox::from_id_salt("mention_mode")
                    .selected_text(match cfg.mention_mode {
                        MentionModeCfg::Manual => "M0 Manual",
                        MentionModeCfg::Assisted => "M1 Assisted",
                        MentionModeCfg::AutoVerified => "M2 Auto verified",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut cfg.mention_mode, MentionModeCfg::Manual, "M0 Manual");
                        ui.selectable_value(&mut cfg.mention_mode, MentionModeCfg::Assisted, "M1 Assisted");
                        ui.selectable_value(&mut cfg.mention_mode, MentionModeCfg::AutoVerified, "M2 Auto verified");
                    });
            });
            ui.weak("M1/M2 chỉ có hiệu lực khi hồ sơ ứng dụng đã được QA trên Zalo thật; hiện mọi chế độ đều hạ về M0.");
            ui.checkbox(&mut cfg.require_same_chat, "Yêu cầu cùng cuộc trò chuyện");
            ui.add(egui::Slider::new(&mut cfg.session_timeout_seconds, 15..=600).text("Hết hạn phiên (giây)"));
            ui.separator();
            ui.checkbox(&mut cfg.autostart, "Khởi động cùng Windows");
        });
        if *cfg != before {
            cmds.push(Cmd::ConfigChanged);
        }
    }

    // ---- Diagnostics -------------------------------------------------------------------------

    fn page_diagnostics(&mut self, ui: &mut egui::Ui, cfg: &Config, st: &EngineStatus) {
        ui.heading("Chẩn đoán");
        ui.label(format!(
            "Hook bàn phím: {}",
            if st.hook_running {
                "chạy"
            } else {
                "không chạy"
            }
        ));
        ui.label(format!("Tạm dừng: {}", st.paused));
        ui.label(format!("Hàng đợi tràn (đã fail-open): {}", st.overflow));
        ui.label(format!(
            "Hook phải cài lại (watchdog): {}",
            st.hook_reinstalls
        ));
        ui.label(format!("Phím đi qua IME (bỏ cửa sổ gõ): {}", st.ime_keys));
        ui.label(format!(
            "Chế độ mention hiệu lực: {}",
            st.mention_effective_mode
        ));
        ui.label(format!(
            "Phiên hiện tại: {}",
            if st.session_status.is_empty() {
                "—"
            } else {
                &st.session_status
            }
        ));
        for p in &st.hotkey_problems {
            ui.colored_label(ERR, p);
        }
        ui.separator();
        ui.strong("Ma trận tương thích Zalo PC");
        ui.label(format!("strict_focus = {}", cfg.strict_focus));
        ui.label("text_paste: untested · multiline: untested · mention_manual: untested · mention_assisted: untested · mention_auto_verified: disabled");
        ui.weak("Chưa có bằng chứng thử trên Zalo PC thật. Xem docs/compatibility-matrix.md.");
        ui.separator();
        ui.strong("Nhật ký (chỉ trạng thái, không có nội dung)");
        egui::ScrollArea::vertical()
            .id_salt("diag_log")
            .max_height(240.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for l in &st.log {
                    ui.monospace(l);
                }
            });
    }
}

fn hotkey_row(ui: &mut egui::Ui, label: &str, v: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::TextEdit::singleline(v).desired_width(180.0));
        if zs_hotkey_valid(v) {
            ui.colored_label(OK, "hợp lệ");
        } else {
            ui.colored_label(ERR, "không hợp lệ (cần ít nhất 1 phím bổ trợ)");
        }
    });
}

/// Light syntax check mirroring the platform parser (modifier + key).
fn zs_hotkey_valid(s: &str) -> bool {
    let parts: Vec<String> = s
        .split('+')
        .map(|p| p.trim().to_ascii_lowercase())
        .filter(|p| !p.is_empty())
        .collect();
    let is_mod = |p: &str| matches!(p, "ctrl" | "control" | "alt" | "shift" | "win" | "super");
    let mods = parts.iter().filter(|p| is_mod(p)).count();
    let keys: Vec<&String> = parts.iter().filter(|p| !is_mod(p)).collect();
    mods >= 1
        && keys.len() == 1
        && (matches!(keys[0].as_str(), "space" | "enter" | "tab")
            || (keys[0].len() == 1 && keys[0].as_bytes()[0].is_ascii_alphanumeric())
            || (keys[0].starts_with('f')
                && keys[0][1..]
                    .parse::<u32>()
                    .is_ok_and(|n| (1..=24).contains(&n))))
}

fn type_name(t: FieldType) -> &'static str {
    match t {
        FieldType::Text => "text",
        FieldType::Multiline => "multiline",
        FieldType::Mention => "mention",
    }
}

fn split_tags(s: &str) -> Vec<String> {
    let mut v: Vec<String> = s
        .split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    v.sort();
    v.dedup();
    v
}

fn save_edit(edit: &mut Edit, store: &mut Store, cache: &[Snippet]) -> Result<(), String> {
    edit.s.code = edit.s.code.trim().to_string();
    if !is_valid_code(&edit.s.code) {
        return Err(
            "Mã không hợp lệ: bắt đầu bằng chữ cái, 2–32 ký tự gồm chữ, số, _ hoặc -".into(),
        );
    }
    if cache
        .iter()
        .any(|o| o.id != edit.s.id && o.code.eq_ignore_ascii_case(&edit.s.code))
    {
        return Err(format!("Trùng mã '{}'", edit.s.code));
    }
    if edit.s.body.trim().is_empty() {
        return Err("Nội dung trống".into());
    }
    let tpl = zs_core::Template::parse(&edit.s.body)
        .map_err(|e| format!("Dòng {}, cột {}: {}", e.line, e.col, e.kind))?;
    if edit.s.title.trim().is_empty() {
        edit.s.title = edit.s.code.clone();
    }
    edit.s.tags = split_tags(&edit.tags);
    // Drop metadata for placeholders that no longer exist.
    edit.s
        .fields
        .retain(|f| tpl.fields.iter().any(|t| t.key.as_str() == f.key));
    store.save(&edit.s).map_err(|e| e.to_string())
}

fn iso_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // civil_from_days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_now_shape() {
        let s = iso_now();
        assert_eq!(s.len(), 20);
        assert!(s.starts_with("20") && s.ends_with('Z'));
    }

    #[test]
    fn hotkey_syntax_check() {
        assert!(zs_hotkey_valid("Ctrl+Alt+Space"));
        assert!(zs_hotkey_valid("ctrl+alt+shift+m"));
        assert!(!zs_hotkey_valid("Space"));
        assert!(!zs_hotkey_valid("Ctrl+Alt"));
        assert!(!zs_hotkey_valid("Ctrl+A+B"));
    }

    #[test]
    fn save_edit_validates() {
        let mut store = Store::open_in_memory().unwrap();
        let mut e = Edit::from(Snippet::new("a", "t", "x"), true);
        assert!(save_edit(&mut e, &mut store, &[]).is_err()); // code too short
        e.s.code = "abc".into();
        e.s.body = "bad [oops".into();
        assert!(
            save_edit(&mut e, &mut store, &[])
                .unwrap_err()
                .contains("Dòng 1")
        );
        e.s.body = "ok [Tên]".into();
        assert!(save_edit(&mut e, &mut store, &[]).is_ok());
        let other = Snippet::new("ABC", "t", "y");
        let mut e2 = Edit::from(Snippet::new("abc", "t", "z"), true);
        assert!(
            save_edit(&mut e2, &mut store, &[other])
                .unwrap_err()
                .contains("Trùng")
        );
    }
}
