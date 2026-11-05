//! The Info tab: detected setup, resource links and version footer.

use iced::widget::{column, row};
use iced::{Element, Length};

use super::widgets::{card, kv, muted, ornament, link_btn, section_title, LINK_NEXUS, LINK_PLAYGUIDE, LINK_WIKI};
use super::{App, Message};
use crate::paths::APP_VERSION;

pub fn info_tab(app: &App) -> Element<'_, Message> {
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
