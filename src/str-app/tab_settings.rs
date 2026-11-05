//! The Settings tab: Nexus API key, game folder override, launch options,
//! offline archives and profiles.

use iced::widget::{checkbox, column, pick_list, row, text_input};
use iced::{Element, Length};

use super::widgets::{card, danger_btn, link_btn, muted, ornament, secondary_btn, section_title, LINK_APIKEY};
use super::view::offline_row;
use super::{App, Message, OpenTarget};
use crate::config::LaunchMode;
use crate::ui;

pub fn settings_tab(app: &App) -> Element<'_, Message> {
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
                LaunchMode::ALL,
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
            secondary_btn("OPEN GAME FOLDER", Message::OpenFolder(OpenTarget::Game)).on_press_maybe(
                app.game_dir.is_some().then_some(Message::OpenFolder(OpenTarget::Game)),
            ),
            secondary_btn("OPEN DOWNLOADS", Message::OpenFolder(OpenTarget::Downloads))
                .on_press_maybe(
                    app.dirs.is_some().then_some(Message::OpenFolder(OpenTarget::Downloads)),
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
