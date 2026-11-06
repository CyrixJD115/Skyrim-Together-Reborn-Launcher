//! STR Launcher — a universal, fully automated Skyrim Together Reborn launcher
//! for Windows and Linux.
//!
//! Themed after the Skyrim main menu: near-black backdrop, carved-caps
//! Cinzel headings, parchment-and-gold accents and the Skyrim Together dragon
//! emblem.

// GUI app: never allocate a console window on Windows.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

// Each `str-<group>/` directory below `src/` holds one area of the launcher;
// `#[path]` keeps the crate-internal module names short (`crate::steam`,
// `crate::config`, …) while the file tree stays grouped and hyphenated.
#[path = "str-app/mod.rs"]
mod app;
#[path = "str-archive/mod.rs"]
mod archive;
#[path = "str-game/checks.rs"]
mod checks;
#[path = "str-game/cc_clean.rs"]
mod cc_clean;
#[path = "str-config/mod.rs"]
mod config;
#[path = "str-nexus/download.rs"]
mod download;
#[path = "str-launch/mod.rs"]
mod launch;
#[path = "str-nexus/mod.rs"]
mod nexus;
#[path = "str-config/paths.rs"]
mod paths;
#[path = "str-game/pe_version.rs"]
mod pe_version;
#[path = "str-platform/mod.rs"]
mod platform;
#[path = "str-game/plugins.rs"]
mod plugins;
#[path = "str-steam/proton.rs"]
mod proton;
#[path = "str-server/mod.rs"]
mod server;
#[cfg(all(unix, not(target_os = "macos")))]
#[path = "str-entry/sgdb.rs"]
mod sgdb;
#[cfg(all(unix, not(target_os = "macos")))]
#[path = "str-entry/shortcuts_vdf.rs"]
mod shortcuts_vdf;
#[path = "str-steam/mod.rs"]
mod steam;
#[cfg(all(unix, not(target_os = "macos")))]
#[path = "str-entry/mod.rs"]
mod steam_entry;
#[path = "str-install/mod.rs"]
mod str_install;
#[path = "str-ui/mod.rs"]
mod ui;
#[path = "str-steam/vdf.rs"]
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

/// The corner dragon art is composed to bleed off the frame at this size, so
/// the window may resize freely *within* these bounds but never past them —
/// in particular it can never grow large enough for the art to re-crop.
const WINDOW_MAX: iced::Size = iced::Size::new(1020.0, 820.0);
/// Smallest readable layout: content scrolls below this, never squashes.
const WINDOW_MIN: iced::Size = iced::Size::new(880.0, 640.0);

fn main() -> iced::Result {
    iced::application(app::boot, app::update, app::view)
        .title(title)
        .theme(dark_theme)
        .font(include_bytes!("../assets/fonts/Cinzel-Regular.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Cinzel-Bold.ttf").as_slice())
        .window(iced::window::Settings {
            size: WINDOW_MAX,
            min_size: Some(WINDOW_MIN),
            max_size: Some(WINDOW_MAX),
            resizable: true,
            icon: window_icon(),
            ..Default::default()
        })
        .run()
}
