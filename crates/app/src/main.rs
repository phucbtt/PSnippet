//! PSnippet — offline text expander with Zalo mention orchestration (composition root).
#![windows_subsystem = "windows"]

mod logo;
mod orchestrator;
mod tray;

use zs_core::Config;
use zs_storage::Store;

fn data_dir() -> std::path::PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let new = base.join("PSnippet");
    let old = base.join("ZaloSnippet");
    // Migrate data from the pre-rename folder; if it is locked (old build still running) keep using it.
    if !new.exists() && old.exists() && std::fs::rename(&old, &new).is_err() {
        return old.join("data");
    }
    new.join("data")
}

fn main() {
    // Logging: warn+ only by default, never content (policy §18.3). `ZS_LOG_FILE` + `ZS_LOG=info`
    // enable a state-only trace for diagnostics/QA.
    let level = match std::env::var("ZS_LOG").as_deref() {
        Ok("info") => tracing::Level::INFO,
        Ok("debug") => tracing::Level::DEBUG,
        _ => tracing::Level::WARN,
    };
    match std::env::var_os("ZS_LOG_FILE").and_then(|p| std::fs::File::create(p).ok()) {
        Some(f) => tracing_subscriber::fmt()
            .with_max_level(level)
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(f))
            .init(),
        None => tracing_subscriber::fmt().with_max_level(level).init(),
    }

    let Some(_instance) = zs_platform_windows::system::acquire_single_instance() else {
        // Another instance owns the tray; nothing else to do.
        return;
    };
    let background = std::env::args().any(|a| a == "--background");

    let dir = data_dir();
    let mut store = match Store::open(&dir.join("snippets.db")) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("cannot open database: {e}");
            return;
        }
    };
    let _ = store.seed_if_empty();
    let cfg: Config = store
        .get_setting("config")
        .ok()
        .flatten()
        .unwrap_or_default();

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("PSnippet")
            .with_icon(eframe::egui::IconData {
                rgba: logo::rgba(64, logo::BLUE),
                width: 64,
                height: 64,
            })
            .with_inner_size([980.0, 680.0])
            .with_min_inner_size([560.0, 360.0])
            .with_visible(!background),
        ..Default::default()
    };
    let result = eframe::run_native(
        "PSnippet",
        options,
        Box::new(move |cc| Ok(Box::new(orchestrator::ZsApp::new(cc, store, cfg)))),
    );
    if let Err(e) = result {
        tracing::error!("ui error: {e}");
    }
    // Hooks and tray are released in `on_exit`; make sure no background thread keeps the process alive.
    std::process::exit(0);
}
