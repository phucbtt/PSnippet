//! Application coordinator: wires hook events → compose session FSM → UI → platform injection.
//! Safety rules enforced here (spec §1.3, §3.2, §6A.4): no Send/Enter, two-phase insert (trigger stays
//! until confirm), re-validate the target before touching it, never blind-Backspace, copy-only fallback.

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use tray_icon::menu::{MenuEvent, MenuId};
use zs_core::{
    Config, Delim, FieldKey, FieldType, FieldValues, MentionModeCfg, RenderSegment, Snippet,
    render_plain_text_preview,
};
use zs_mention::{
    Action, ComposeSession, ComposeState, ContextVerdict, Event, MentionMode, Status,
    TargetContext, check_context, effective_mode,
};
use zs_platform_windows as pw;
use zs_platform_windows::hook::{HookConfig, HookEvent, HookHandle, HotkeyAction};
use zs_platform_windows::inject::PasteSession;
use zs_platform_windows::uia::FocusProbe;
use zs_storage::Store;
use zs_ui::egui::{self, ViewportCommand};
use zs_ui::{
    Cmd, EngineStatus, FormOutcome, FormState, MainView, PaletteOutcome, PaletteState,
    SessionOutcome, SessionView, show_form, show_palette, show_session_panel,
};

use crate::tray::Tray;

pub enum AppEvent {
    Hook(HookEvent),
    Menu(MenuId),
    Captured(Option<String>),
}

enum Mode {
    Main,
    Form(Box<FormState>),
    Palette,
    Notice(SessionView),
}

struct Pending {
    target: Option<TargetContext>,
    trigger_len: usize,
    /// Static snippet: runs straight through with no user wait (context verified moments ago).
    immediate: bool,
    session: ComposeSession,
    paste: Option<PasteSession>,
    awaiting_since: Option<Instant>,
    snippet_code: String,
    notice: Option<SessionView>,
    /// User ticked "this is the right chat" in the form (needed when the chat cannot be verified).
    chat_ack: bool,
}

pub struct ZsApp {
    store: Store,
    cfg: Config,
    view: MainView,
    status: EngineStatus,
    rx: Receiver<AppEvent>,
    tx: Sender<AppEvent>,
    hook: Option<HookHandle>,
    tray: Option<Tray>,
    mode: Mode,
    pending: Option<Pending>,
    palette: PaletteState,
    palette_target: Option<TargetContext>,
    palette_cache: Vec<Snippet>,
    own_hwnd: isize,
    quit: bool,
    activate_next_frame: bool,
    hotkey_apply_at: Option<Instant>,
    applied_hotkeys: (String, String, String, String),
    started: Instant,
    log: VecDeque<String>,
}

/// Settings-table key prefix for remembered form values (opt-in, see `Config::remember_field_values`).
const FIELDVALS_PREFIX: &str = "fieldvals:";

fn own_hwnd(cc: &eframe::CreationContext<'_>) -> isize {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match cc.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
        _ => 0,
    }
}

impl ZsApp {
    pub fn new(cc: &eframe::CreationContext<'_>, store: Store, cfg: Config) -> Self {
        zs_ui::install_fonts(&cc.egui_ctx);
        zs_ui::apply_theme(&cc.egui_ctx);
        let (tx, rx) = channel();
        let hwnd = own_hwnd(cc);
        pw::context::set_ignore_hwnd(hwnd);

        // Tray menu events → app channel (+ wake the UI loop, which may be hidden).
        {
            let (tx, ctx) = (tx.clone(), cc.egui_ctx.clone());
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                let _ = tx.send(AppEvent::Menu(e.id));
                ctx.request_repaint();
            }));
        }
        zs_ui::install_fonts(&cc.egui_ctx);
        let tray = Tray::new(cfg.autostart, cfg.zalo_only).ok();
        let mut app = Self {
            store,
            cfg,
            view: MainView::default(),
            status: EngineStatus::default(),
            rx,
            tx,
            hook: None,
            tray,
            mode: Mode::Main,
            pending: None,
            palette: PaletteState::fresh(),
            palette_target: None,
            palette_cache: Vec::new(),
            own_hwnd: hwnd,
            quit: false,
            activate_next_frame: false,
            hotkey_apply_at: None,
            applied_hotkeys: (String::new(), String::new(), String::new(), String::new()),
            started: Instant::now(),
            log: VecDeque::new(),
        };
        app.start_hook(&cc.egui_ctx);
        app.refresh_quick();
        if app.tray.is_none() {
            app.log("tray: không tạo được biểu tượng khay");
        }
        app
    }

    fn log(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        tracing::info!("{msg}");
        let line = format!("[{:>6}s] {}", self.started.elapsed().as_secs(), msg);
        if self.log.len() >= 200 {
            self.log.pop_front();
        }
        self.log.push_back(line);
    }

    fn tooltip(&self, text: &str) {
        if let Some(t) = &self.tray {
            t.set_tooltip(text);
        }
    }

    // ---- hook lifecycle ------------------------------------------------------------------------

    fn codes(&self) -> Vec<String> {
        self.store
            .all()
            .map(|v| {
                v.into_iter()
                    .filter(|s| s.enabled)
                    .map(|s| s.code)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn hotkeys(&self) -> Vec<(HotkeyAction, String)> {
        vec![
            (HotkeyAction::Palette, self.cfg.palette_hotkey.clone()),
            (HotkeyAction::Resume, self.cfg.mention_resume_hotkey.clone()),
            (
                HotkeyAction::CancelSession,
                self.cfg.mention_cancel_hotkey.clone(),
            ),
            (HotkeyAction::TogglePause, self.cfg.toggle_hotkey.clone()),
        ]
    }

    fn start_hook(&mut self, ctx: &egui::Context) {
        if let Some(mut h) = self.hook.take() {
            h.stop();
        }
        self.status.hotkey_problems.clear();
        let cfg = HookConfig {
            matcher: self.cfg.matcher_config(),
            codes: self.codes(),
            allowed_apps: self.cfg.effective_allowed_apps(),
            hotkeys: self.hotkeys(),
        };
        match HookHandle::start(cfg) {
            Ok((h, rx)) => {
                h.shared.set_paused(!self.cfg.hotstring_enabled);
                self.status.paused = !self.cfg.hotstring_enabled;
                self.hook = Some(h);
                let (tx, ctx) = (self.tx.clone(), ctx.clone());
                std::thread::spawn(move || {
                    for ev in rx {
                        if tx.send(AppEvent::Hook(ev)).is_err() {
                            break;
                        }
                        ctx.request_repaint();
                    }
                });
                self.applied_hotkeys = (
                    self.cfg.palette_hotkey.clone(),
                    self.cfg.mention_resume_hotkey.clone(),
                    self.cfg.mention_cancel_hotkey.clone(),
                    self.cfg.toggle_hotkey.clone(),
                );
                self.log("hook: đã khởi động");
            }
            Err(e) => self.log(format!("hook: LỖI {e}")),
        }
        if let Some(t) = &self.tray {
            t.set_paused(self.status.paused);
        }
    }

    fn apply_config(&mut self, ctx: &egui::Context) {
        let _ = self.store.set_setting("config", &self.cfg);
        if !self.cfg.remember_field_values {
            // Turning the feature off also forgets what was stored (privacy default).
            let _ = self.store.delete_settings_with_prefix(FIELDVALS_PREFIX);
        }
        if let Some(h) = &self.hook {
            h.shared.set_matcher_config(self.cfg.matcher_config());
            h.shared.set_allowed_apps(self.cfg.effective_allowed_apps());
            let codes = self.codes();
            h.shared.set_codes(codes.iter().map(String::as_str));
            h.shared
                .set_paused(!self.cfg.hotstring_enabled || self.status.paused);
        }
        let hk = (
            self.cfg.palette_hotkey.clone(),
            self.cfg.mention_resume_hotkey.clone(),
            self.cfg.mention_cancel_hotkey.clone(),
            self.cfg.toggle_hotkey.clone(),
        );
        if hk != self.applied_hotkeys {
            self.hotkey_apply_at = Some(Instant::now() + Duration::from_millis(900)); // debounce typing
        }
        if let Some(t) = &self.tray {
            if t.autostart.is_checked() != self.cfg.autostart {
                t.autostart.set_checked(self.cfg.autostart);
            }
            if t.zalo_only.is_checked() != self.cfg.zalo_only {
                t.zalo_only.set_checked(self.cfg.zalo_only);
            }
        }
        if let Err(e) = pw::system::set_autostart(self.cfg.autostart) {
            self.log(format!("autostart: {e}"));
        }
        let _ = ctx;
    }

    /// Favorites (max 10) shown in the tray's "Quick snippets" submenu.
    fn refresh_quick(&mut self) {
        let items: Vec<(String, String)> = self
            .store
            .all()
            .map(|v| {
                v.into_iter()
                    .filter(|s| s.enabled && s.favorite)
                    .take(10)
                    .map(|s| (s.code, s.title))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(t) = &mut self.tray {
            t.set_quick(&items);
        }
    }

    fn on_quick_snippet(&mut self, code: &str, ctx: &egui::Context) {
        let Ok(Some(s)) = self.store.get_by_code(code) else {
            return;
        };
        let Ok(t) = s.template() else { return };
        if t.fields.is_empty() {
            // Static: copy the rendered text; the user pastes it themselves. Nothing is typed or sent.
            if let Ok(segs) = t.render(&FieldValues::new()) {
                let _ = pw::clipboard::set_text(&render_plain_text_preview(&segs));
                self.tooltip(&format!("PSnippet: đã sao chép '{code}'."));
            }
        } else if self.pending.is_none() && matches!(self.mode, Mode::Main) {
            // No verified target (the tray click moved focus) => form is copy-only.
            self.begin_compose(s, None, 0, ctx, false);
        }
    }

    fn reload_codes(&mut self) {
        let codes = self.codes();
        if let Some(h) = &self.hook {
            h.shared.set_codes(codes.iter().map(String::as_str));
        }
        self.view.invalidate();
    }

    fn set_paused(&mut self, paused: bool) {
        self.status.paused = paused;
        if let Some(h) = &self.hook {
            h.shared.set_paused(paused);
        }
        if let Some(t) = &self.tray {
            t.set_paused(paused);
        }
        self.log(if paused {
            "tạm dừng"
        } else {
            "tiếp tục"
        });
    }

    // ---- window helpers ------------------------------------------------------------------------

    fn show_window(&mut self, ctx: &egui::Context, size: [f32; 2], on_top: bool) {
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(size[0], size[1])));
        ctx.send_viewport_cmd(ViewportCommand::WindowLevel(if on_top {
            egui::WindowLevel::AlwaysOnTop
        } else {
            egui::WindowLevel::Normal
        }));
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        self.activate_next_frame = true;
        ctx.request_repaint();
    }

    fn hide_window(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
    }

    fn show_main(&mut self, ctx: &egui::Context) {
        self.mode = Mode::Main;
        self.show_window(ctx, [980.0, 680.0], false);
    }

    // ---- events --------------------------------------------------------------------------------

    fn drain_events(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                AppEvent::Hook(h) => self.on_hook_event(h, ctx),
                AppEvent::Menu(id) => self.on_menu(id, ctx),
                AppEvent::Captured(exe) => match exe {
                    Some(p)
                        if !self
                            .cfg
                            .allowed_apps
                            .iter()
                            .any(|a| a.eq_ignore_ascii_case(&p)) =>
                    {
                        self.cfg.allowed_apps.push(p.clone());
                        self.apply_config(ctx);
                        self.view.set_app_message(format!("Đã thêm: {p}"));
                    }
                    Some(p) => self
                        .view
                        .set_app_message(format!("Đã có trong danh sách: {p}")),
                    None => self.view.set_app_message(
                        "Không xác định được ứng dụng (có thể chạy với quyền cao hơn).",
                    ),
                },
            }
        }
    }

    fn on_menu(&mut self, id: MenuId, ctx: &egui::Context) {
        let Some(t) = &self.tray else { return };
        if let Some(code) = t
            .quick
            .iter()
            .find(|(qid, _)| *qid == id)
            .map(|(_, c)| c.clone())
        {
            self.on_quick_snippet(&code, ctx);
        } else if id == *t.zalo_only.id() {
            self.cfg.zalo_only = t.zalo_only.is_checked();
            self.apply_config(ctx);
        } else if id == *t.open.id() {
            self.show_main(ctx);
        } else if id == *t.diagnostics.id() {
            self.view.page = zs_ui::Page::Diagnostics;
            self.show_main(ctx);
        } else if id == *t.palette.id() {
            self.open_palette(ctx, false);
        } else if id == *t.pause.id() {
            let p = !self.status.paused;
            self.set_paused(p);
        } else if id == *t.autostart.id() {
            self.cfg.autostart = t.autostart.is_checked();
            self.apply_config(ctx);
        } else if id == *t.exit.id() {
            self.quit = true;
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    fn on_hook_event(&mut self, ev: HookEvent, ctx: &egui::Context) {
        match ev {
            HookEvent::Trigger { m, hwnd } => self.on_trigger(m, hwnd, ctx),
            HookEvent::Hotkey(HotkeyAction::Palette) => self.open_palette(ctx, true),
            HookEvent::Hotkey(HotkeyAction::Resume) => self.resume(ctx),
            HookEvent::Hotkey(HotkeyAction::TogglePause) => {
                let p = !self.status.paused;
                self.set_paused(p);
                self.tooltip(if p {
                    "PSnippet: ĐÃ TẮT gõ tắt"
                } else {
                    "PSnippet: ĐÃ BẬT gõ tắt"
                });
            }
            HookEvent::Hotkey(HotkeyAction::CancelSession) => {
                if self.awaiting() {
                    self.feed(Event::CancelRemaining, ctx);
                }
            }
            HookEvent::HotkeyUnavailable(a) => {
                self.status.hotkey_problems.push(format!("Không đăng ký được phím tắt {a:?}: đã bị ứng dụng khác chiếm. Hãy đổi trong Settings."));
                self.log(format!("hotkey {a:?}: không khả dụng"));
            }
            HookEvent::Overflow => self.log("hook: hàng đợi đầy, đã fail-open"),
        }
    }

    fn probe_timeout(&self) -> Duration {
        Duration::from_millis(self.cfg.ui_probe_timeout_ms.max(100))
    }

    /// Re-emit the swallowed delimiter when we decide not to act, so the user's typing is unchanged.
    fn restore_delimiter(m: &zs_core::Match) {
        let _ = match m.delimiter {
            Some(Delim::Space) => pw::inject::space(),
            Some(Delim::Tab) => pw::inject::tab(),
            _ => Ok(()),
        };
    }

    fn evaluate_probe(&self, probe: &Option<FocusProbe>) -> Result<(), &'static str> {
        if probe.as_ref().is_some_and(|p| p.is_password) {
            return Err("password field");
        }
        // Instant (UniKey-style) expansion works wherever the user can type; only password fields are refused.
        if self.cfg.strict_focus
            && !self.cfg.instant_expand
            && !probe.as_ref().is_some_and(|p| p.is_editable)
        {
            return Err("E_FOCUS_UNKNOWN");
        }
        Ok(())
    }

    fn on_trigger(&mut self, m: zs_core::Match, hwnd: isize, ctx: &egui::Context) {
        if self.pending.is_some() || !matches!(self.mode, Mode::Main) {
            // One session at a time (MN-34).
            Self::restore_delimiter(&m);
            self.log(format!("bỏ qua trigger '{}': đang có phiên khác", m.code));
            return;
        }
        let snippet = match self.store.get_by_code(&m.code) {
            Ok(Some(s)) if s.enabled => s,
            _ => {
                Self::restore_delimiter(&m);
                return;
            }
        };
        let Some((target, probe)) = pw::context::snapshot_foreground(self.probe_timeout()) else {
            Self::restore_delimiter(&m);
            self.log("E_FOCUS_UNKNOWN: không lấy được cửa sổ đang dùng");
            return;
        };
        if target.hwnd != hwnd {
            Self::restore_delimiter(&m);
            self.log("E_TARGET_CHANGED: cửa sổ đổi ngay khi kích hoạt");
            return;
        }
        if let Err(why) = self.evaluate_probe(&probe) {
            Self::restore_delimiter(&m);
            self.log(format!("từ chối '{}': {why}", m.code));
            self.tooltip(&format!(
                "PSnippet: không chèn ({why}). Dùng palette {} để sao chép.",
                self.cfg.palette_hotkey
            ));
            return;
        }
        self.log(format!("trigger '{}' ({} ký tự)", m.code, m.trigger_len));
        self.begin_compose(
            snippet,
            Some(target),
            m.trigger_len,
            ctx,
            m.delimiter == Some(Delim::Space),
        );
    }

    // ---- compose flow --------------------------------------------------------------------------

    fn session_mode(&self) -> MentionMode {
        // No profile has QA evidence yet ⇒ everything clamps to Manual (spec §6A.2, §12A.4).
        let requested = match self.cfg.mention_mode {
            MentionModeCfg::Manual => MentionMode::Manual,
            MentionModeCfg::Assisted => MentionMode::Assisted,
            MentionModeCfg::AutoVerified => MentionMode::AutoVerified,
        };
        effective_mode(requested, zs_mention::MentionCapability::Unverified)
    }

    fn begin_compose(
        &mut self,
        snippet: Snippet,
        target: Option<TargetContext>,
        trigger_len: usize,
        ctx: &egui::Context,
        space_delim: bool,
    ) {
        let tpl = match snippet.template() {
            Ok(t) => t,
            Err(e) => {
                self.log(format!("E_PARSER_INVALID {}: {e}", snippet.code));
                return;
            }
        };
        // Palette picks (trigger_len 0) keep the form; typed triggers expand in place when instant_expand is on.
        let instant = self.cfg.instant_expand && trigger_len > 0;
        let immediate = tpl.fields.is_empty() || instant;
        let mut session = ComposeSession::new(self.session_mode());
        let _ = session.handle(Event::TriggerMatched);
        let _ = session.handle(Event::OpenForm);
        let code = snippet.code.clone();
        self.pending = Some(Pending {
            target,
            trigger_len,
            immediate,
            session,
            paste: None,
            awaiting_since: None,
            snippet_code: code,
            notice: None,
            chat_ack: false,
        });
        if immediate {
            let rendered = if tpl.fields.is_empty() {
                tpl.render(&FieldValues::new()).ok()
            } else {
                Some(tpl.render_placeholders())
            };
            match rendered {
                Some(mut segs) => {
                    if self.cfg.preserve_delimiter
                        && space_delim
                        && let Some(RenderSegment::Text(t)) = segs.last_mut()
                    {
                        t.push(' ');
                    }
                    self.feed(Event::ConfirmValid(segs), ctx);
                }
                None => self.pending = None,
            }
        } else {
            let mut form = FormState::new(snippet, tpl);
            self.prefill_remembered(&mut form);
            if self.pending.as_ref().is_some_and(|p| p.target.is_none()) {
                form.banner =
                    Some("Không có vùng nhập hợp lệ: chỉ có thể sao chép bản nháp.".into());
                form.insert_enabled = false;
            } else if self.cfg.strict_focus {
                // Chat identity is not provable yet: require an explicit tick instead of silently
                // refusing (which looked like "nothing was inserted").
                form.needs_chat_ack = true;
                form.banner = Some(
                    "Hãy kiểm tra đúng cuộc trò chuyện, tick xác nhận bên dưới rồi chèn (hoặc bấm Sao chép).".into(),
                );
            } else {
                form.banner = Some(
                    "Chế độ tương thích: hãy kiểm tra đúng cuộc trò chuyện trước khi chèn.".into(),
                );
            }
            self.mode = Mode::Form(Box::new(form));
            self.show_window(ctx, [560.0, 560.0], true);
        }
    }

    /// Prefill non-mention fields with the last confirmed values (only when the user opted in).
    fn prefill_remembered(&self, form: &mut FormState) {
        if !self.cfg.remember_field_values {
            return;
        }
        let key = format!("{FIELDVALS_PREFIX}{}", form.snippet.id);
        let Ok(Some(saved)) = self.store.get_setting::<HashMap<FieldKey, String>>(&key) else {
            return;
        };
        let fields: Vec<_> = form
            .template
            .fields
            .iter()
            .filter(|f| f.field_type != FieldType::Mention)
            .map(|f| f.key.clone())
            .collect();
        for k in fields {
            if let Some(v) = saved.get(&k) {
                form.values.set(&k, v.clone());
            }
        }
    }

    /// Persist the confirmed text of non-mention fields (never person names to tag).
    fn remember_form_values(&mut self) {
        if !self.cfg.remember_field_values {
            return;
        }
        let Mode::Form(f) = &self.mode else { return };
        let map: HashMap<FieldKey, String> = f
            .template
            .fields
            .iter()
            .filter(|fd| fd.field_type != FieldType::Mention)
            .map(|fd| (fd.key.clone(), f.values.text(&fd.key).to_string()))
            .collect();
        let key = format!("{FIELDVALS_PREFIX}{}", f.snippet.id);
        let _ = self.store.set_setting(&key, &map);
    }

    fn awaiting(&self) -> bool {
        self.pending.as_ref().is_some_and(|p| {
            matches!(
                p.session.state(),
                ComposeState::AwaitMentionSelection | ComposeState::ManualAssist
            )
        })
    }

    /// Feed an event to the session and execute resulting actions (synchronously chaining
    /// platform results back in as events).
    fn feed(&mut self, ev: Event, ctx: &egui::Context) {
        let Some(p) = self.pending.as_mut() else {
            return;
        };
        let actions = match p.session.handle(ev) {
            Ok(a) => a,
            Err(e) => {
                let msg = format!("sự kiện không hợp lệ: {e}");
                self.log(msg);
                return;
            }
        };
        let mut queue: VecDeque<Action> = actions.into();
        while let Some(a) = queue.pop_front() {
            let follow: Vec<Action> = match a {
                Action::OpenForm => vec![],
                Action::RevalidateTarget => {
                    let ev = self.revalidate_for_confirm();
                    self.step(ev)
                }
                Action::RemoveTrigger => {
                    let ev = self.remove_trigger();
                    self.step(ev)
                }
                Action::InjectText(t) => {
                    let ev = self.inject_text(&t);
                    self.step(ev)
                }
                Action::RequestMention {
                    query,
                    index,
                    total,
                } => {
                    self.on_request_mention(&query, index, total);
                    vec![]
                }
                Action::CopyText { text, .. } => {
                    match pw::clipboard::set_text(&text) {
                        Ok(_) => {
                            if let Some(p) = self.pending.as_mut() {
                                p.notice
                                    .get_or_insert_with(|| notice("Đã dừng", "", true))
                                    .remaining = Some(text);
                            }
                        }
                        Err(e) => self.log(format!("E_CLIPBOARD_BUSY: {e}")),
                    }
                    vec![]
                }
                Action::Show(s) => {
                    self.on_status(s);
                    vec![]
                }
            };
            for f in follow.into_iter().rev() {
                queue.push_front(f);
            }
        }
        self.after_event(ctx);
    }

    fn step(&mut self, ev: Event) -> Vec<Action> {
        match self.pending.as_mut().map(|p| p.session.handle(ev)) {
            Some(Ok(a)) => a,
            _ => vec![],
        }
    }

    /// Terminal-state cleanup: finish clipboard restore, surface notices, drop the session.
    fn after_event(&mut self, ctx: &egui::Context) {
        let Some(p) = self.pending.as_mut() else {
            return;
        };
        if !p.session.state().is_terminal() {
            return;
        }
        let state = p.session.state();
        let mut p = self.pending.take().expect("checked above");
        if let Some(ps) = p.paste.take() {
            ps.finish();
        }
        self.log(format!("phiên '{}' kết thúc: {state:?}", p.snippet_code));
        self.status.session_status.clear();
        match (state, p.notice.take()) {
            (ComposeState::DraftCompleted, _) => {
                // Do not steal focus from the Zalo draft.
                self.hide_window(ctx);
                self.mode = Mode::Main;
            }
            (ComposeState::CancelledUnchanged | ComposeState::AbortedSafe, _) => {
                self.hide_window(ctx);
                self.mode = Mode::Main;
            }
            (_, Some(n)) => {
                self.mode = Mode::Notice(n);
                self.show_window(ctx, [520.0, 260.0], true);
            }
            (_, None) => {
                self.hide_window(ctx);
                self.mode = Mode::Main;
            }
        }
    }

    fn live_context(&self) -> Option<(TargetContext, Option<FocusProbe>)> {
        pw::context::snapshot_foreground(self.probe_timeout())
    }

    /// Re-validation right after the user confirms the form (or immediately for static snippets).
    fn revalidate_for_confirm(&mut self) -> Event {
        let Some(p) = &self.pending else {
            return Event::TargetInvalid;
        };
        let Some(target) = p.target.clone() else {
            return Event::TargetInvalid;
        };
        let immediate = p.immediate;
        let chat_ack = p.chat_ack;
        if !pw::context::focus_window(target.hwnd) {
            self.log("revalidate: không đưa được cửa sổ đích về foreground (E_TARGET_CHANGED)");
            return Event::TargetInvalid;
        }
        let Some((live, probe)) = self.live_context() else {
            self.log("revalidate: không đọc được ngữ cảnh hiện tại");
            return Event::TargetInvalid;
        };
        if probe.as_ref().is_some_and(|p| p.is_password) {
            self.log("revalidate: ô mật khẩu");
            return Event::TargetInvalid;
        }
        let verdict = check_context(&target, &live);
        if !matches!(verdict, ContextVerdict::Same) {
            self.log(format!(
                "revalidate: {verdict:?} (gen {}→{}, chat_ack={})",
                target.generation, live.generation, chat_ack
            ));
        }
        match verdict {
            ContextVerdict::Same => Event::TargetValid,
            // Chat identity can't be proven. A static snippet runs within milliseconds of the verified
            // trigger; a form involved user wait time, so strict mode must not guess.
            ContextVerdict::Unverifiable if immediate || !self.cfg.strict_focus || chat_ack => {
                Event::TargetValid
            }
            _ => Event::TargetInvalid,
        }
    }

    fn remove_trigger(&mut self) -> Event {
        let n = self.pending.as_ref().map_or(0, |p| p.trigger_len);
        if n == 0 {
            return Event::TriggerRemoved;
        }
        match pw::inject::backspaces(n) {
            Ok(()) => Event::TriggerRemoved,
            Err(e) => {
                self.log(format!("E_TRIGGER_UNSAFE: {e}"));
                Event::TriggerRemoveFailed
            }
        }
    }

    fn inject_text(&mut self, text: &str) -> Event {
        let restore = self.cfg.clipboard_restore != "never";
        let Some(p) = self.pending.as_mut() else {
            return Event::InjectFailed;
        };
        // Focus/generation guard right before every injection (no blind keystrokes).
        if let Some(t) = &p.target {
            let still = pw::context::foreground_hwnd().is_some_and(|h| h.0 as isize == t.hwnd)
                && pw::context::generation() == t.generation;
            if !still {
                return Event::InjectFailed;
            }
        }
        let ps = p.paste.get_or_insert_with(|| PasteSession::begin(restore));
        match ps.paste(text) {
            Ok(()) => Event::SegmentInjected,
            Err(e) => {
                self.log(format!("inject: {e}"));
                Event::InjectFailed
            }
        }
    }

    fn on_request_mention(&mut self, query: &str, index: usize, total: usize) {
        if let Some(p) = self.pending.as_mut() {
            p.awaiting_since = Some(Instant::now());
        }
        let msg = format!(
            "Chờ chọn người {}/{}: gõ @ rồi tên \"{}\" trong Zalo, chọn đúng thành viên, rồi nhấn {}",
            index + 1,
            total,
            query,
            self.cfg.mention_resume_hotkey
        );
        self.status.session_status = format!("Chờ chọn người {}/{} ({})", index + 1, total, query);
        self.tooltip(&format!("PSnippet: {msg}"));
        self.log(format!("chờ tag {}/{}", index + 1, total));
    }

    fn on_status(&mut self, s: Status) {
        match s {
            Status::WaitingMention { .. } => {}
            Status::ManualAssist(code) => {
                self.tooltip(&format!("PSnippet: {}", code.message_vi()));
                self.log(code.code());
            }
            Status::DraftReady {
                mentions,
                verification,
            } => {
                let v = match verification {
                    zs_mention::MentionVerification::UserConfirmed => {
                        format!("{mentions} tag do bạn xác nhận (máy chưa xác minh)")
                    }
                    zs_mention::MentionVerification::VerifiedRichMention => {
                        format!("{mentions} tag đã xác minh")
                    }
                    _ => "không có tag".to_string(),
                };
                self.tooltip(&format!(
                    "PSnippet: Đã chuẩn bị bản nháp ({v}). Vui lòng kiểm tra trước khi gửi."
                ));
                self.log("DraftCompleted (chưa gửi)");
            }
            Status::Aborted(code) => {
                self.log(code.code());
                let n = notice("Đã dừng để an toàn", code.message_vi(), true);
                if let Some(p) = self.pending.as_mut() {
                    p.notice = Some(n);
                }
            }
        }
    }

    fn resume(&mut self, ctx: &egui::Context) {
        if !self.awaiting() {
            return;
        }
        let Some(target) = self.pending.as_ref().and_then(|p| p.target.clone()) else {
            return;
        };
        let ok = pw::context::focus_window(target.hwnd)
            && self.live_context().is_some_and(|(live, probe)| {
                // Resume is the user's attestation that they picked the member in this chat (§6A.10);
                // it can't override a provably different window/process/focus.
                !probe.as_ref().is_some_and(|p| p.is_password)
                    && check_context(&target, &live) != ContextVerdict::Changed
            });
        self.feed(Event::MentionUserConfirmed { same_context: ok }, ctx);
    }

    fn tick(&mut self, ctx: &egui::Context) {
        if let Some(at) = self.hotkey_apply_at
            && Instant::now() >= at
        {
            self.hotkey_apply_at = None;
            self.start_hook(ctx);
        }
        let timeout = Duration::from_secs(self.cfg.session_timeout_seconds);
        if self.awaiting() {
            ctx.request_repaint_after(Duration::from_secs(1));
            let expired = self
                .pending
                .as_ref()
                .and_then(|p| p.awaiting_since)
                .is_some_and(|t| t.elapsed() > timeout);
            if expired {
                self.feed(Event::Timeout, ctx);
            } else if let Some(t) = self.pending.as_ref().and_then(|p| p.target.as_ref()) {
                // Foreground moved to another app/window while waiting ⇒ stop (MN-12/MN-32).
                let gen_changed = pw::context::generation() != t.generation;
                let gone = !pw::context::window_exists(t.hwnd);
                if gen_changed || gone {
                    self.feed(Event::TargetChanged, ctx);
                }
            }
        }
        self.status.hook_running = self.hook.is_some();
        self.status.overflow = self.hook.as_ref().map_or(0, |h| h.shared.overflow_count());
        self.status.hook_reinstalls = self.hook.as_ref().map_or(0, |h| h.shared.reinstall_count());
        self.status.ime_keys = self.hook.as_ref().map_or(0, |h| h.shared.ime_key_count());
        self.status.log = self.log.iter().cloned().collect();
        self.status.foreground_exe = None;
        self.status.mention_effective_mode = format!("{:?}", self.session_mode());
    }

    // ---- palette -------------------------------------------------------------------------------

    fn open_palette(&mut self, ctx: &egui::Context, from_hotkey: bool) {
        if self.pending.is_some() {
            return;
        }
        self.palette_target = None;
        if from_hotkey {
            // Snapshot the target BEFORE our window takes focus.
            if let Some((t, probe)) = pw::context::snapshot_foreground(self.probe_timeout()) {
                let allowed = self
                    .cfg
                    .allowed_apps
                    .iter()
                    .any(|a| pw::context::exe_matches(&t.executable_path, a));
                if allowed && self.evaluate_probe(&probe).is_ok() {
                    self.palette_target = Some(t);
                }
            }
        }
        self.palette_cache = self.store.all().unwrap_or_default();
        self.palette = PaletteState::fresh();
        self.mode = Mode::Palette;
        self.show_window(ctx, [680.0, 440.0], true);
    }

    // ---- UI ------------------------------------------------------------------------------------

    fn handle_cmds(&mut self, cmds: Vec<Cmd>, ctx: &egui::Context) {
        for c in cmds {
            match c {
                Cmd::ConfigChanged => self.apply_config(ctx),
                Cmd::SnippetsChanged | Cmd::RestoredFromBackup => {
                    self.reload_codes();
                    self.refresh_quick();
                }
                Cmd::TogglePause => {
                    let p = !self.status.paused;
                    self.set_paused(p);
                }
                Cmd::OpenPalette => self.open_palette(ctx, false),
                Cmd::CaptureForegroundApp => {
                    let tx = self.tx.clone();
                    let ctx = ctx.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_secs(3));
                        let _ = tx.send(AppEvent::Captured(pw::context::foreground_exe()));
                        ctx.request_repaint();
                    });
                }
                Cmd::TestSnippet(s) => {
                    if let Ok(t) = s.template() {
                        let mut f = FormState::new(s, t);
                        f.insert_enabled = false;
                        f.banner =
                            Some("Chế độ thử: chỉ xem trước và sao chép, không chèn.".into());
                        self.mode = Mode::Form(Box::new(f));
                        self.pending = None;
                    }
                }
            }
        }
    }

    fn session_view(&self) -> Option<SessionView> {
        let p = self.pending.as_ref()?;
        if !self.awaiting() {
            return None;
        }
        Some(SessionView {
            title: format!("Phiên chèn '{}'", p.snippet_code),
            status: self.status.session_status.clone(),
            instruction: format!(
                "Gõ @ rồi tên trong Zalo, chọn đúng thành viên từ danh sách gợi ý của Zalo, sau đó nhấn {}.",
                self.cfg.mention_resume_hotkey
            ),
            resume_hotkey: self.cfg.mention_resume_hotkey.clone(),
            finished: false,
            error: false,
            remaining: Some(p.session.remaining_text()),
        })
    }
}

fn notice(title: &str, message: &str, error: bool) -> SessionView {
    SessionView {
        title: title.into(),
        status: message.into(),
        instruction: "Nội dung chưa chèn đã được sao chép vào clipboard (mention không được giữ — chỉ là văn bản minh họa). Hãy dán thủ công vào đúng cuộc trò chuyện.".into(),
        resume_hotkey: String::new(),
        finished: true,
        error,
        remaining: None,
    }
}

impl eframe::App for ZsApp {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &root.ctx().clone();
        if ctx.input(|i| i.viewport().close_requested()) && !self.quit {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            // Closing the window never stops the engine (US-11): hide, keep tray + hooks alive.
            if self
                .pending
                .as_ref()
                .is_some_and(|p| matches!(p.session.state(), ComposeState::AwaitFields))
            {
                self.feed(Event::Cancel, ctx);
            }
            self.mode = Mode::Main;
            self.hide_window(ctx);
        }
        if self.activate_next_frame {
            self.activate_next_frame = false;
            pw::inject::activate_own_window(self.own_hwnd);
        }
        self.drain_events(ctx);
        self.tick(ctx);

        let mut cmds: Vec<Cmd> = Vec::new();
        let mut form_outcome = FormOutcome::None;
        let mut palette_outcome = PaletteOutcome::None;
        let mut session_outcome = SessionOutcome::None;
        let mut dismiss_notice = false;
        let sess_view = self.session_view();
        let live_preview = self.cfg.live_preview;

        egui::CentralPanel::default().show(root, |ui| match &mut self.mode {
            Mode::Main => {
                if let Some(v) = &sess_view {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        session_outcome = show_session_panel(ui, v);
                    });
                    ui.add_space(6.0);
                }
                cmds = self
                    .view
                    .show(ui, &mut self.store, &mut self.cfg, &self.status);
            }
            Mode::Form(f) => form_outcome = show_form(ui, f, live_preview),
            Mode::Palette => {
                palette_outcome = show_palette(ui, &mut self.palette, &self.palette_cache)
            }
            Mode::Notice(n) => {
                if matches!(
                    show_session_panel(ui, n),
                    SessionOutcome::Dismiss | SessionOutcome::CopyRemaining(_)
                ) {
                    dismiss_notice = true;
                }
            }
        });

        self.handle_cmds(cmds, ctx);
        match session_outcome {
            SessionOutcome::Resume => self.resume(ctx),
            SessionOutcome::CancelRemaining => self.feed(Event::CancelRemaining, ctx),
            SessionOutcome::CopyRemaining(t) => {
                let _ = pw::clipboard::set_text(&t);
            }
            _ => {}
        }
        if dismiss_notice {
            self.mode = Mode::Main;
            self.hide_window(ctx);
        }
        match form_outcome {
            FormOutcome::None => {}
            FormOutcome::Insert(segs, ack) => {
                if let Some(p) = self.pending.as_mut() {
                    p.chat_ack = ack;
                }
                self.remember_form_values();
                self.mode = Mode::Main;
                self.hide_window(ctx); // let the target regain focus before we touch it
                self.feed(Event::ConfirmValid(segs), ctx);
            }
            FormOutcome::Copy(text) => {
                self.remember_form_values();
                let _ = pw::clipboard::set_text(&text);
                self.tooltip("PSnippet: đã sao chép bản nháp (mention là văn bản minh họa).");
                if self.pending.is_some() {
                    self.feed(Event::Cancel, ctx);
                } else {
                    self.mode = Mode::Main;
                }
                self.hide_window(ctx);
            }
            FormOutcome::Cancel => {
                // Trigger text stays exactly as typed (two-phase rule).
                if self.pending.is_some() {
                    self.feed(Event::Cancel, ctx);
                }
                self.mode = Mode::Main;
                self.hide_window(ctx);
            }
        }
        match palette_outcome {
            PaletteOutcome::None => {}
            PaletteOutcome::Close => {
                self.mode = Mode::Main;
                self.hide_window(ctx);
            }
            PaletteOutcome::Copy(s) => {
                if let Ok(t) = s.template() {
                    if t.fields.is_empty() {
                        if let Ok(segs) = t.render(&FieldValues::new()) {
                            let _ = pw::clipboard::set_text(&render_plain_text_preview(&segs));
                        }
                    } else {
                        let _ = pw::clipboard::set_text(&s.body);
                    }
                }
                self.mode = Mode::Main;
                self.hide_window(ctx);
            }
            PaletteOutcome::Pick(s) => {
                self.mode = Mode::Main;
                let target = self.palette_target.take();
                self.hide_window(ctx);
                self.begin_compose(s, target, 0, ctx, false);
            }
        }
    }

    fn on_exit(&mut self) {
        // Truly stop: unhook, unregister hotkeys, drop tray.
        if let Some(mut h) = self.hook.take() {
            h.stop();
        }
        self.tray = None;
    }
}
