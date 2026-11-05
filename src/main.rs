//! STR Launcher — a universal, fully automated Skyrim Together Reborn launcher
//! for Windows and Linux.
//!
//! Themed after the Skyrim main menu: near-black backdrop, carved-caps
//! Cinzel headings, parchment-and-gold accents and the Skyrim Together dragon
//! emblem.

mod app;
mod archive;
mod cc_clean;
mod checks;
mod config;
mod download;
mod launch;
mod nexus;
mod paths;
mod pe_version;
mod platform;
mod plugins;
mod proton;
mod server;
mod sgdb;
mod shortcuts_vdf;
mod steam;
#[cfg(all(unix, not(target_os = "macos")))]
mod steam_entry;
mod str_install;
mod ui;
mod vdf;

/// The Skyrim Together dragon emblem (detailed), used for the window icon.
const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

/// Public handle for platform code that needs the emblem on disk (e.g. the
/// Linux desktop entry icon).
pub const LOGO_ICON_PNG: &[u8] = ICON_PNG;

fn window_icon() -> Option<iced::window::icon::Icon> {
    let img = image::load_from_memory(ICON_PNG).ok()?.to_rgba8();
    let (width, height) = img.dimensions();
    iced::window::icon::from_rgba(img.into_raw(), width, height).ok()
}

fn title(_app: &app::App) -> String {
    "STR Launcher".to_string()
}

fn dark_theme(_app: &app::App) -> iced::Theme {
    iced::Theme::Dark
}

fn main() -> iced::Result {
    iced::application(app::boot, app::update, app::view)
        .title(title)
        .theme(dark_theme)
        .font(include_bytes!("../assets/fonts/Cinzel-Regular.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Cinzel-Bold.ttf").as_slice())
        .window(iced::window::Settings {
            size: iced::Size::new(1020.0, 820.0),
            min_size: Some(iced::Size::new(880.0, 640.0)),
            icon: window_icon(),
            ..Default::default()
        })
        .run()
}
