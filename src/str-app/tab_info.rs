//! The Info tab: detected setup, resource links and version footer.

use iced::widget::{column, row, text};
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

    // Game-version compatibility matrix from the version manifest.
    let matrix = crate::manifest::embedded();
    if !matrix.game_versions.is_empty() {
        let mut compat = column![].spacing(6);
        compat = compat.push(section_title("Compatibility"));
        compat = compat.push(muted(format!(
            "Skyrim Together Reborn {} needs an SSE runtime of {} or newer.",
            matrix
                .str
                .versions
                .keys()
                .next_back()
                .map(String::as_str)
                .unwrap_or("1.8"),
            matrix.min_runtime.as_deref().unwrap_or("1.6.640"),
        )));
        for row_data in &matrix.game_versions {
            let (color, mark) = if row_data.supported {
                (crate::ui::SUCCESS_BRIGHT, "✓")
            } else {
                (crate::ui::DANGER_BRIGHT, "✕")
            };
            let supports = if row_data.str_versions.is_empty() {
                "no STR version".to_string()
            } else {
                format!("STR {}", row_data.str_versions.join(", "))
            };
            compat = compat.push(
                row![
                    text(mark).size(11).color(color),
                    text(format!("runtime {}", row_data.runtime))
                        .size(13)
                        .color(crate::ui::TEXT),
                    text(supports).size(12).color(crate::ui::GOLD),
                    text(row_data.note.clone())
                        .size(12)
                        .color(crate::ui::MUTED)
                        .width(Length::Fill)
                        .wrapping(iced::widget::text::Wrapping::Word),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            );
        }
        page = page.push(card(compat));
    }

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
