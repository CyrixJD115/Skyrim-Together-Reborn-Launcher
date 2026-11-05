//! Rendering for the app — modelled on the Skyrim main menu: a white dragon
//! emblem over near-black, gold accents and carved-caps Cinzel headings. The
//! top bar holds the four sections so each screen stays short and focused.

use iced::widget::{button, checkbox, column, container, pick_list, progress_bar, row, scrollable, space, svg, text, text_input};
use iced::{color, Element, Length};

use super::{App, Banner, Message, OpenTarget, Phase, ServerField, Tab};
use crate::paths::APP_VERSION;
use crate::server::INI_HINT;
use crate::ui::{self, FONT_CINZEL, FONT_CINZEL_BOLD};

const LINK_NEXUS: &str = "https://www.nexusmods.com/skyrimspecialedition/mods/69993";
const LINK_WIKI: &str = "https://wiki.tiltedphoques.com/tilted-online/guides/getting-started";
const LINK_PLAYGUIDE: &str = "https://wiki.tiltedphoques.com/tilted-online/general-information/playguide";
const LINK_APIKEY: &str = "https://www.nexusmods.com/settings/api-keys";

/// The solid-white Skyrim Together dragon (vector), bundled into the binary.
const LOGO_SVG: &[u8] = include_bytes!("../../assets/logo.svg");
/// Same dragon, baked at ~5% opacity for the corner watermark.
const WATERMARK_SVG: &[u8] = include_bytes!("../../assets/watermark.svg");

/// Content width shared by the header and every tab.
const CONTENT_WIDTH: f32 = 820.0;

const TABS: [(Tab, &str); 4] = [
    (Tab::Client, "CLIENT"),
    (Tab::Server, "SERVER"),
    (Tab::Info, "INFO"),
    (Tab::Settings, "SETTINGS"),
];

pub fn view(app: &App) -> Element<'_, Message> {
    let mut root = column![header(app)].spacing(0);

    if let Some(banner) = banner(app) {
        root = root.push(
            container(column![banner].max_width(CONTENT_WIDTH))
                .padding(iced::Padding { top: 10.0, right: 26.0, bottom: 0.0, left: 26.0 })
                .center_x(Length::Fill)
                .width(Length::Fill),
        );
    }

    root = root.push(tab_page(app));

    // The dragon watermark sits behind nothing — it is layered *on top* of
    // the UI but is fully transparent to mouse events (svg never captures
    // them), so every click passes through to the widgets underneath.
    let main = container(root)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(ui::screen);

    iced::widget::stack![main, watermark()].into()
}

/// A big, faint dragon emblem pinned to the bottom-right corner.
fn watermark() -> Element<'static, Message> {
    let dragon = svg(svg::Handle::from_memory(WATERMARK_SVG.to_vec()))
        .width(Length::Fixed(230.0))
        .height(Length::Fixed(338.0));
    container(dragon)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Bottom)
        .padding(iced::Padding { right: 30.0, bottom: 22.0, ..Default::default() })
        .into()
}

fn tab_page(app: &App) -> Element<'_, Message> {
    let body = match app.tab {
        Tab::Client => client_tab(app),
        Tab::Server => server_tab(app),
        Tab::Info => info_tab(app),
        Tab::Settings => settings_tab(app),
    };

    let content = column![body].spacing(14).max_width(CONTENT_WIDTH);
    let padded = container(content)
        .padding(iced::Padding { top: 14.0, right: 26.0, bottom: 26.0, left: 26.0 })
        .center_x(Length::Fill);

    scrollable(padded)
        .id(tab_scroll_id(app.tab))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(ui::scrollable_style)
        .into()
}

fn tab_scroll_id(tab: Tab) -> &'static str {
    match tab {
        Tab::Client => "tab-client",
        Tab::Server => "tab-server",
        Tab::Info => "tab-info",
        Tab::Settings => "tab-settings",
    }
}

// ---------------------------------------------------------------------------
// Top bar
// ---------------------------------------------------------------------------

fn header(app: &App) -> Element<'_, Message> {
    // Brand: small dragon + wordmark + version, above the tab row.
    let brand = row![
        svg(svg::Handle::from_memory(LOGO_SVG.to_vec()))
            .width(Length::Fixed(22.0))
            .height(Length::Fixed(32.0)),
        column![
            text("STR LAUNCHER").size(17).font(FONT_CINZEL_BOLD).color(ui::TEXT),
            text(format!("MULTIPLAYER LAUNCHER · v{APP_VERSION}"))
                .size(10)
                .font(FONT_CINZEL)
                .color(ui::GOLD),
        ]
        .spacing(1),
    ]
    .spacing(12)
    .align_y(iced::Alignment::Center);

    let inner = column![
        row![brand, space::horizontal(), status_chip(app)],
        row![
            tabs(app),
            space::horizontal(),
            secondary_btn("RE-DETECT", Message::Detect)
                .on_press_maybe((!app.busy()).then_some(Message::Detect)),
        ]
        .spacing(14)
        .align_y(iced::Alignment::Center),
        ornament(),
    ]
    .spacing(10)
    .max_width(CONTENT_WIDTH);

    container(inner)
        .padding(iced::Padding { top: 14.0, right: 26.0, bottom: 6.0, left: 26.0 })
        .center_x(Length::Fill)
        .width(Length::Fill)
        .into()
}

fn status_chip(app: &App) -> Element<'static, Message> {
    let (color, label) = match app.phase {
        Phase::Idle if app.installed() => (ui::SUCCESS_BRIGHT, "READY"),
        Phase::Idle if app.game_dir.is_some() => (ui::GOLD, "SETUP NEEDED"),
        Phase::Idle => (ui::DANGER_BRIGHT, "GAME NOT FOUND"),
        Phase::Detecting => (ui::GOLD_BRIGHT, "DETECTING"),
        Phase::Installing => (ui::GOLD_BRIGHT, "INSTALLING"),
        Phase::Undoing => (ui::DANGER_BRIGHT, "REMOVING"),
        Phase::GettingServer => (ui::GOLD_BRIGHT, "DOWNLOADING"),
    };

    row![
        text("◆").size(10).color(color),
        text(label).size(11).font(FONT_CINZEL_BOLD).color(color),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center)
    .into()
}

fn tabs(app: &App) -> Element<'static, Message> {
    let mut bar = row![].spacing(6);
    for (tab, label) in TABS {
        bar = bar.push(tab_btn(label, tab, app.tab));
    }
    bar.into()
}

fn tab_btn(label: &'static str, tab: Tab, active: Tab) -> iced::widget::Button<'static, Message> {
    button(
        text(label)
            .size(12)
            .font(FONT_CINZEL_BOLD),
    )
    .padding([8, 16])
    .style(ui::tab_button(tab == active))
    .on_press(Message::TabSelected(tab))
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

fn client_tab(app: &App) -> Element<'_, Message> {
    let can = !app.busy();
    let installed = app.installed();
    let mut status = column![].spacing(12);
    status = status.push(
        text(app.status_line())
            .size(19)
            .color(ui::TEXT)
            .wrapping(iced::widget::text::Wrapping::Word),
    );

    if app.busy() {
        status = status.push(progress_block(app));
    } else if app.game_dir.is_none() {
        status = status.push(muted(
            "Auto-detection is Steam-only. Using GOG, Epic or Heroic? Set the game folder under Settings.",
        ));
    }

    status = status.push(ornament());

    let mut actions = row![].spacing(10);
    if installed {
        actions = actions.push(
            go_btn("▶  LAUNCH TOGETHER", Message::LaunchTogether)
                .on_press_maybe(can.then_some(Message::LaunchTogether)),
        );
        actions = actions.push(
            secondary_btn("REINSTALL", Message::Install)
                .on_press_maybe(can.then_some(Message::Install)),
        );
    } else {
        actions = actions.push(
            primary_btn("INSTALL SKYRIM TOGETHER", Message::Install)
                .on_press_maybe(can.then_some(Message::Install)),
        );
    }
    actions = actions.push(
        secondary_btn("LAUNCH VANILLA", Message::LaunchVanilla)
            .on_press_maybe((can && app.game_dir.is_some()).then_some(Message::LaunchVanilla)),
    );
    if installed {
        actions = actions.push(
            danger_btn("UNINSTALL", Message::Undo)
                .on_press_maybe(can.then_some(Message::Undo)),
        );
    }
    status = status.push(actions.wrap());

    let mut page = column![card(status)].spacing(16);
    if let Some(preflight) = preflight_card(app) {
        page = page.push(preflight);
    }
    if let Some(log) = activity_log(app) {
        page = page.push(log);
    }
    page.into()
}

/// The launch-readiness checklist: every precondition for a successful
/// launch, probed live so failures are visible before they happen.
fn preflight_card(app: &App) -> Option<Element<'_, Message>> {
    if app.game_dir.is_none() {
        return None;
    }

    let mut rows = column![].spacing(8);
    for check in &app.checks {
        let (glyph, color) = if check.ok {
            ("✔", ui::SUCCESS_BRIGHT)
        } else {
            ("✘", ui::DANGER_BRIGHT)
        };
        rows = rows.push(
            row![
                text(glyph).size(13).color(color).width(Length::Fixed(18.0)),
                column![
                    text(check.label).size(13).color(ui::TEXT),
                    text(check.detail.clone())
                        .size(12)
                        .color(ui::MUTED)
                        .wrapping(iced::widget::text::Wrapping::Word),
                ]
                .spacing(1)
                .width(Length::Fill),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        );
    }

    let failing = app.checks.iter().filter(|c| !c.ok).count();
    let footnote = if failing == 0 {
        muted("Everything checks out — press LAUNCH TOGETHER (or start it from Steam).")
    } else {
        muted(format!(
            "{failing} item(s) need attention — Install fixes all of them automatically."
        ))
    };

    Some(
        card(
            column![
                row![section_title("Launch readiness"), space::horizontal()],
                rows,
                ornament(),
                footnote,
            ]
            .spacing(10),
        )
        .into(),
    )
}

fn progress_block(app: &App) -> Element<'_, Message> {
    let mut col = column![].spacing(6);

    if matches!(app.phase, Phase::Installing | Phase::Undoing) && app.step_total > 0 {
        col = col.push(
            row![
                text(format!(
                    "STEP {} OF {}",
                    (app.step_index + 1).min(app.step_total),
                    app.step_total
                ))
                .size(11)
                .font(FONT_CINZEL_BOLD)
                .color(ui::GOLD),
                space::horizontal(),
                text(app.step_label.clone()).size(12).color(ui::MUTED),
            ]
            .width(Length::Fill)
            .align_y(iced::Alignment::Center),
        );
    }

    if let Some((label, frac)) = &app.progress {
        col = col.push(
            row![
                text(label.clone()).size(12).color(ui::MUTED),
                space::horizontal(),
                text(format!("{}%", (frac * 100.0).round() as u64))
                    .size(12)
                    .color(ui::MUTED),
            ]
            .width(Length::Fill),
        );
        col = col.push(
            container(progress_bar(0.0..=1.0, *frac).style(ui::progress_bar))
                .height(Length::Fixed(6.0))
                .width(Length::Fill),
        );
    }

    col.into()
}

fn activity_log(app: &App) -> Option<Element<'_, Message>> {
    if app.log.is_empty() {
        return None;
    }

    let mut lines = column![].spacing(3);
    for line in &app.log {
        lines = lines.push(
            text(line.clone())
                .size(12)
                .font(iced::Font::MONOSPACE)
                .color(ui::MUTED),
        );
    }

    Some(
        column![
            section_title("Activity"),
            container(
                scrollable(lines)
                    .height(Length::Fixed(150.0))
                    .width(Length::Fill)
                    .style(ui::scrollable_style),
            )
            .padding(10)
            .width(Length::Fill)
            .style(ui::card_flat),
        ]
        .spacing(8)
        .into(),
    )
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

fn server_tab(app: &App) -> Element<'_, Message> {
    let can = !app.busy();
    let mut page = column![].spacing(14);

    let mut head = column![].spacing(10);
    head = head.push(
        row![
            section_title("Dedicated server"),
            space::horizontal(),
            installed_chip(app.server_installed),
        ]
        .align_y(iced::Alignment::Center),
    );
    head = head.push(muted(
        "Host your own world instead of relying on a friend. Players join with the in-game F2 menu → Connect.",
    ));
    head = head.push(
        row![
            secondary_btn("GET LATEST SERVER", Message::ServerGet)
                .on_press_maybe(can.then_some(Message::ServerGet)),
            secondary_btn("USE OFFLINE ZIP…", Message::BrowseOffline(2))
                .on_press_maybe(can.then_some(Message::BrowseOffline(2))),
            go_btn("LAUNCH SERVER", Message::ServerLaunch)
                .on_press_maybe((can && app.server_installed).then_some(Message::ServerLaunch)),
        ]
        .spacing(10)
        .wrap(),
    );

    if app.phase == Phase::GettingServer {
        head = head.push(progress_block(app));
    }
    if app.offline[2].is_some() {
        head = head.push(offline_row(app, 2, "Server zip"));
    }
    page = page.push(card(head));

    if app.server_installed {
        page = page.push(server_settings_card(app));
    }

    page.into()
}

fn installed_chip(installed: bool) -> Element<'static, Message> {
    let (color, label) = if installed {
        (ui::SUCCESS_BRIGHT, "INSTALLED")
    } else {
        (ui::MUTED, "NOT INSTALLED")
    };

    row![
        text("◆").size(9).color(color),
        text(label).size(11).font(FONT_CINZEL_BOLD).color(color),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center)
    .into()
}

fn server_settings_card(app: &App) -> Element<'_, Message> {
    let mut col = column![].spacing(10);
    col = col.push(section_title("Server settings"));

    match &app.server_ini {
        Some((path, _)) => {
            col = col.push(muted(format!("Editing {}", path.display())));
            col = col.push(server_field(
                "Port",
                "7777",
                &app.server_inputs[0],
                ServerField::Port,
                false,
            ));
            col = col.push(server_field(
                "Server name",
                "My server",
                &app.server_inputs[1],
                ServerField::Name,
                false,
            ));
            col = col.push(server_field(
                "Password",
                "No password",
                &app.server_inputs[2],
                ServerField::Password,
                true,
            ));
            col = col.push(server_field(
                "Max players",
                "8",
                &app.server_inputs[3],
                ServerField::MaxPlayers,
                false,
            ));
            col = col.push(server_field(
                "Mod check",
                "true / false",
                &app.server_inputs[4],
                ServerField::ModCheck,
                false,
            ));
            col = col.push(ornament());
            col = col.push(
                row![
                    go_btn("SAVE SETTINGS", Message::ServerSaveConfig),
                    secondary_btn("RELOAD FROM FILE", Message::ServerReloadConfig)
                        .on_press_maybe((!app.busy()).then_some(Message::ServerReloadConfig)),
                    secondary_btn("OPEN SERVER FOLDER", Message::OpenFolder(OpenTarget::Server))
                        .on_press_maybe(
                            app.server_installed
                                .then_some(Message::OpenFolder(OpenTarget::Server))
                        ),
                ]
                .spacing(8)
                .wrap(),
            );
        }
        None => {
            col = col.push(muted(INI_HINT));
            col = col.push(
                row![
                    secondary_btn("RELOAD FROM FILE", Message::ServerReloadConfig)
                        .on_press_maybe((!app.busy()).then_some(Message::ServerReloadConfig)),
                    secondary_btn("OPEN SERVER FOLDER", Message::OpenFolder(OpenTarget::Server))
                        .on_press_maybe(
                            app.server_installed
                                .then_some(Message::OpenFolder(OpenTarget::Server))
                        ),
                ]
                .spacing(8),
            );
        }
    }

    card(col)
}

fn server_field<'a>(
    label: &'a str,
    placeholder: &'a str,
    value: &'a str,
    field: ServerField,
    secure: bool,
) -> Element<'a, Message> {
    row![
        text(label)
            .size(13)
            .color(ui::MUTED)
            .width(Length::Fixed(130.0)),
        text_input(placeholder, value)
            .on_input(move |v| Message::ServerField(field, v))
            .secure(secure)
            .width(Length::Fill)
            .style(ui::text_input),
    ]
    .spacing(10)
    .align_y(iced::Alignment::Center)
    .into()
}

// ---------------------------------------------------------------------------
// Info
// ---------------------------------------------------------------------------

fn info_tab(app: &App) -> Element<'_, Message> {
    let mut page = column![].spacing(14);

    let mut left = column![].spacing(6);
    let mut right = column![].spacing(6);

    left = left.push(kv(
        "Game folder",
        app.game_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "not found".into()),
    ));
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        left = left.push(kv(
            "Compatibility tool",
            app.proton_selected.clone().unwrap_or_else(|| "auto".into()),
        ));
    }
    left = left.push(kv(
        "Creation Club",
        if app.cc_present {
            "present (cleaned on install)".to_string()
        } else {
            "clean".to_string()
        },
    ));

    right = right.push(kv(
        "Skyrim build",
        app.game_version.clone().unwrap_or_else(|| "unknown".into()),
    ));
    right = right.push(kv(
        "Together Reborn",
        app.str_version
            .as_ref()
            .map(|v| format!("v{v}"))
            .unwrap_or_else(|| "not installed".into()),
    ));
    right = right.push(kv(
        "Address Library",
        app.al_version
            .as_ref()
            .map(|v| format!("v{v}"))
            .unwrap_or_else(|| "not installed".into()),
    ));

    let mut setup = column![].spacing(10);
    setup = setup.push(section_title("Detected setup"));
    setup = setup.push(
        row![
            column![left].width(Length::FillPortion(1)),
            column![right].width(Length::FillPortion(1)),
        ]
        .spacing(24),
    );

    if !app.installs.is_empty() {
        let list = app
            .installs
            .iter()
            .map(|i| i.game_dir.display().to_string())
            .collect::<Vec<_>>()
            .join("  •  ");
        setup = setup.push(muted(format!("Steam libraries with Skyrim: {list}")));
    }
    page = page.push(card(setup));

    let mut links = column![].spacing(10);
    links = links.push(section_title("Resources"));
    links = links.push(muted(
        "Setup help, the multiplayer play guide and the mod pages on Nexus.",
    ));
    links = links.push(
        row![
            link_btn("SKYRIM TOGETHER REBORN ON NEXUS", LINK_NEXUS),
            link_btn("OFFICIAL SETUP GUIDE", LINK_WIKI),
            link_btn("PLAY GUIDE", LINK_PLAYGUIDE),
        ]
        .spacing(6)
        .wrap(),
    );
    page = page.push(card(links));

    page = page.push(ornament());
    page = page.push(muted(format!(
        "Runs fully offline after setup · Works with Steam, Proton and GE-Proton · v{APP_VERSION}"
    )));

    page.into()
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

fn settings_tab(app: &App) -> Element<'_, Message> {
    let can = !app.busy();
    let mut page = column![].spacing(14);

    // Nexus Mods -----------------------------------------------------------
    let mut nexus = column![].spacing(8);
    nexus = nexus.push(section_title("Nexus Mods"));
    nexus = nexus.push(muted(
        "Automatic downloads need a Personal-use API key. Find it under nexusmods.com → Account Settings → API keys.",
    ));
    nexus = nexus.push(
        row![
            text_input("Paste your Nexus API key", &app.api_key)
                .on_input(Message::ApiKeyChanged)
                .secure(true)
                .width(Length::FillPortion(3))
                .style(ui::text_input),
            link_btn("GET A KEY", LINK_APIKEY),
            secondary_btn(
                if app.checking { "CHECKING…" } else { "CHECK UPDATES" },
                Message::CheckUpdates,
            )
            .on_press_maybe((!app.checking && can).then_some(Message::CheckUpdates)),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    );
    page = page.push(card(nexus));

    // Game ----------------------------------------------------------------
    let mut game = column![].spacing(8);
    game = game.push(section_title("Game"));
    game = game.push(muted(
        "Folder containing SkyrimSE.exe. Auto-detection is Steam-only; set it manually for GOG, Epic or Heroic.",
    ));
    game = game.push(
        row![
            text_input("Path to Skyrim Special Edition", &app.game_path_input)
                .on_input(Message::GamePathChanged)
                .on_submit(Message::GamePathSubmitted)
                .width(Length::Fill)
                .style(ui::text_input),
            secondary_btn("BROWSE…", Message::BrowseGameDir)
                .on_press_maybe(can.then_some(Message::BrowseGameDir)),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    );

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        game = game.push(ornament());
        game = game.push(muted(
            "Compatibility tool — GE-Proton 10 is tested; GE-Proton 11 crashes Skyrim Together 1.8.",
        ));
        game = game.push(
            pick_list(
                app.proton_names.clone(),
                app.proton_selected.clone(),
                Message::ProtonPicked,
            )
            .width(Length::Fill)
            .style(ui::pick_list_style),
        );
    }

    game = game.push(ornament());
    game = game.push(
        checkbox(app.cfg.clean_cc)
            .label("Remove bundled Creation Club content (recommended)")
            .on_toggle(Message::ToggleCleanCc)
            .style(ui::checkbox_style),
    );
    game = game.push(
        checkbox(app.cfg.create_shortcuts)
            .label("Create Start Menu / desktop shortcuts")
            .on_toggle(Message::ToggleShortcuts)
            .style(ui::checkbox_style),
    );
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        game = game.push(muted("Launch mode — how multiplayer starts:"));
        game = game.push(
            pick_list(
                crate::config::LaunchMode::ALL,
                Some(app.cfg.launch_mode),
                Message::LaunchModePicked,
            )
            .width(Length::Fill)
            .style(ui::pick_list_style),
        );
        game = game.push(muted(app.cfg.launch_mode.description()));
    }
    game = game.push(ornament());
    game = game.push(
        row![
            secondary_btn("OPEN GAME FOLDER", Message::OpenFolder(OpenTarget::Game))
                .on_press_maybe(
                    app.game_dir
                        .is_some()
                        .then_some(Message::OpenFolder(OpenTarget::Game))
                ),
            secondary_btn("OPEN DOWNLOADS", Message::OpenFolder(OpenTarget::Downloads))
                .on_press_maybe(
                    app.dirs
                        .is_some()
                        .then_some(Message::OpenFolder(OpenTarget::Downloads))
                ),
        ]
        .spacing(8)
        .wrap(),
    );
    page = page.push(card(game));

    // Offline mode --------------------------------------------------------
    let mut offline = column![].spacing(8);
    offline = offline.push(section_title("Offline mode"));
    offline = offline.push(muted(
        "Download the archives manually and select them here; Install will use them instead of the network.",
    ));
    offline = offline.push(offline_row(app, 0, "Skyrim Together Reborn zip"));
    offline = offline.push(offline_row(app, 1, "Address Library zip"));
    page = page.push(card(offline));

    // Profiles ------------------------------------------------------------
    let mut profiles = column![].spacing(8);
    profiles = profiles.push(section_title("Profiles"));
    profiles = profiles.push(muted(
        "Keep separate game folders, tools and versions for different setups.",
    ));
    let names: Vec<String> = app.cfg.profiles.iter().map(|p| p.name.clone()).collect();
    let selected = app.cfg.active().name.clone();
    profiles = profiles.push(
        row![
            pick_list(names, Some(selected), |name: String| {
                Message::ProfilePicked(
                    app.cfg
                        .profiles
                        .iter()
                        .position(|p| p.name == name)
                        .unwrap_or(0),
                )
            })
            .width(Length::FillPortion(2))
            .style(ui::pick_list_style),
            secondary_btn("NEW", Message::ProfileNew),
            danger_btn("DELETE", Message::ProfileDelete),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    );
    page = page.push(card(profiles));

    page.into()
}

fn offline_row<'a>(app: &'a App, which: u8, label: &'a str) -> Element<'a, Message> {
    let path = app.offline[which as usize].as_ref();
    let display = path
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| format!("{label}: not set"));

    row![
        text(display)
            .size(12)
            .color(ui::MUTED)
            .width(Length::Fill)
            .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
        secondary_btn("BROWSE…", Message::BrowseOffline(which))
            .on_press_maybe((!app.busy()).then_some(Message::BrowseOffline(which))),
        secondary_btn("CLEAR", Message::ClearOffline(which))
            .on_press_maybe(path.is_some().then_some(Message::ClearOffline(which))),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center)
    .into()
}

// ---------------------------------------------------------------------------
// Banner
// ---------------------------------------------------------------------------

fn banner(app: &App) -> Option<Element<'_, Message>> {
    let (msg, accent, bright) = match &app.banner {
        Some(Banner::Error(e)) => (e.clone(), ui::DANGER, ui::DANGER_BRIGHT),
        Some(Banner::Info(i)) => (i.clone(), ui::SUCCESS, ui::SUCCESS_BRIGHT),
        None => return None,
    };
    // Key-related errors get a one-click redirect to the API-keys page.
    let needs_key = msg.contains("API key");

    let mut line = row![
        text("◆").size(11).color(bright),
        text(msg)
            .size(13)
            .width(Length::Fill)
            .wrapping(iced::widget::text::Wrapping::Word),
    ]
    .spacing(10)
    .align_y(iced::Alignment::Center);

    if needs_key {
        line = line.push(link_btn("GET A KEY", LINK_APIKEY));
    }

    line = line.push(
        button(text("✕").size(11))
            .padding([3, 8])
            .style(ui::secondary_button())
            .on_press(Message::DismissBanner),
    );

    Some(
        container(line)
            .padding([10, 14])
            .width(Length::Fill)
            .style(move |theme| {
                let mut s = ui::card(theme);
                s.border.color = accent;
                s.border.width = 1.0;
                s
            })
            .into(),
    )
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

fn card<'a>(content: iced::widget::Column<'a, Message>) -> Element<'a, Message> {
    container(content.spacing(10))
        .padding(iced::Padding { top: 16.0, right: 18.0, bottom: 16.0, left: 18.0 })
        .width(Length::Fill)
        .style(ui::card)
        .into()
}

/// A thin gold ornament line, fading at both ends.
fn ornament() -> Element<'static, Message> {
    let empty: iced::widget::Column<'static, Message> = column![];
    container(empty)
        .height(1)
        .width(Length::Fill)
        .style(ui::gold_rule())
        .into()
}

fn section_title(t: &str) -> Element<'static, Message> {
    row![
        text("◆").size(10).color(ui::GOLD),
        space::horizontal().width(8),
        text(t.to_uppercase()).size(15).font(FONT_CINZEL_BOLD).color(ui::GOLD),
    ]
    .into()
}

fn muted(t: impl Into<String>) -> Element<'static, Message> {
    text(t.into()).size(13).color(ui::MUTED).into()
}

fn kv(label: &str, value: impl Into<String>) -> Element<'static, Message> {
    row![
        text(format!("{label}:  ")).size(13).color(ui::MUTED),
        text(value.into())
            .size(13)
            .color(ui::TEXT)
            .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
    ]
    .into()
}

// ---------------------------------------------------------------------------
// Buttons / links
// ---------------------------------------------------------------------------

fn primary_btn(label: impl Into<String>, msg: Message) -> iced::widget::Button<'static, Message> {
    button(text(label.into()).size(15).font(FONT_CINZEL_BOLD))
        .padding([12, 22])
        .style(ui::primary_button())
        .on_press(msg)
}

fn go_btn(label: impl Into<String>, msg: Message) -> iced::widget::Button<'static, Message> {
    button(text(label.into()).size(15).font(FONT_CINZEL_BOLD))
        .padding([12, 22])
        .style(ui::go_button())
        .on_press(msg)
}

fn secondary_btn(label: impl Into<String>, msg: Message) -> iced::widget::Button<'static, Message> {
    button(text(label.into()).size(12).font(FONT_CINZEL_BOLD))
        .padding([10, 17])
        .style(ui::secondary_button())
        .on_press(msg)
}

fn danger_btn(label: impl Into<String>, msg: Message) -> iced::widget::Button<'static, Message> {
    button(text(label.into()).size(12).font(FONT_CINZEL_BOLD))
        .padding([10, 17])
        .style(ui::danger_button())
        .on_press(msg)
}

fn link_btn(label: &str, url: &'static str) -> iced::widget::Button<'static, Message> {
    button(
        text(label.to_string())
            .size(12)
            .font(FONT_CINZEL_BOLD)
            .color(color!(0xa8, 0x8b, 0x5c)),
    )
    .padding([8, 10])
    .style(|_t, s| iced::widget::button::Style {
        background: None,
        text_color: match s {
            iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed => {
                ui::GOLD_BRIGHT
            }
            _ => color!(0xa8, 0x8b, 0x5c),
        },
        border: iced::Border::default(),
        shadow: Default::default(),
        snap: false,
    })
    .on_press(Message::OpenLink(url))
}
