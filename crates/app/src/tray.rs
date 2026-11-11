//! System tray icon + menu (spec §5.2).

use tray_icon::menu::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

pub struct Tray {
    pub icon: TrayIcon,
    pub open: MenuItem,
    pub palette: MenuItem,
    pub quick_menu: Submenu,
    /// (menu id, snippet code) for the current "Quick snippets" entries.
    pub quick: Vec<(MenuId, String)>,
    pub pause: MenuItem,
    pub zalo_only: CheckMenuItem,
    pub autostart: CheckMenuItem,
    pub diagnostics: MenuItem,
    pub exit: MenuItem,
}

/// 32×32 PSnippet logo; grey while paused.
fn make_icon(paused: bool) -> Icon {
    let bg = if paused {
        crate::logo::GREY
    } else {
        crate::logo::BLUE
    };
    Icon::from_rgba(crate::logo::rgba(32, bg), 32, 32).expect("valid icon buffer")
}

impl Tray {
    pub fn new(autostart: bool, zalo_only: bool) -> Result<Self, String> {
        let open = MenuItem::new("Mở PSnippet", true, None);
        let palette = MenuItem::new("Mở palette mẫu", true, None);
        let pause = MenuItem::new("Tạm dừng", true, None);
        let quick_menu = Submenu::new("Mẫu nhanh", true);
        let zalo_only = CheckMenuItem::new("Phạm vi: chỉ Zalo", true, zalo_only, None);
        let autostart = CheckMenuItem::new("Khởi động cùng Windows", true, autostart, None);
        let diagnostics = MenuItem::new("Chẩn đoán", true, None);
        let exit = MenuItem::new("Thoát", true, None);
        let menu = Menu::new();
        menu.append_items(&[
            &open,
            &palette,
            &quick_menu,
            &PredefinedMenuItem::separator(),
            &pause,
            &zalo_only,
            &autostart,
            &diagnostics,
            &PredefinedMenuItem::separator(),
            &exit,
        ])
        .map_err(|e| e.to_string())?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("PSnippet")
            .with_icon(make_icon(false))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            icon,
            open,
            palette,
            quick_menu,
            quick: Vec::new(),
            pause,
            zalo_only,
            autostart,
            diagnostics,
            exit,
        })
    }

    /// Replace the "Quick snippets" entries (code, title). Clicking one copies/opens it (never sends).
    pub fn set_quick(&mut self, items: &[(String, String)]) {
        while self.quick_menu.remove_at(0).is_some() {}
        self.quick.clear();
        if items.is_empty() {
            let empty = MenuItem::new("(chưa có mẫu yêu thích)", false, None);
            let _ = self.quick_menu.append(&empty);
            return;
        }
        for (code, title) in items {
            let mi = MenuItem::new(format!("{code} — {title}"), true, None);
            if self.quick_menu.append(&mi).is_ok() {
                self.quick.push((mi.id().clone(), code.clone()));
            }
        }
    }

    pub fn set_tooltip(&self, text: &str) {
        let _ = self.icon.set_tooltip(Some(text));
    }

    pub fn set_paused(&self, paused: bool) {
        self.pause.set_text(if paused {
            "Tiếp tục"
        } else {
            "Tạm dừng"
        });
        let _ = self.icon.set_icon(Some(make_icon(paused)));
    }
}
