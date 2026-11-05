//! The Client tab: status, progress, primary actions, the launch-readiness
//! checklist and the activity log.

use iced::widget::{column, row, space, text};
use iced::{Element, Length};

use super::widgets::{card, danger_btn, go_btn, muted, ornament, primary_btn, secondary_btn, section_title};
use super::{view::progress_block, App, Message};
use crate::ui::{self, FONT_CINZEL_BOLD};

pub fn client_tab(app: &App) -> Element<'_, Message> {
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
            danger_btn("UNINSTALL", Message::Undo).on_press_maybe(can.then_some(Message::Undo)),
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

/// The launch-readiness checklist as a collapsible section: collapsed it is
/// a one-line summary ("all clear" / "N need attention"); expanding it shows
/// every precondition, probed live so failures are visible before they
/// happen instead of failing silently.
fn preflight_card(app: &App) -> Option<Element<'_, Message>> {
    use iced::widget::button;

    // Nothing to preflight without a game folder.
    app.game_dir.as_ref()?;

    let failing = app.checks.iter().filter(|c| !c.ok).count();
    let (glyph, color, summary) = if failing == 0 {
        ("✔", ui::SUCCESS_BRIGHT, "all clear".to_string())
    } else {
        ("✘", ui::DANGER_BRIGHT, format!("{failing} need attention"))
    };
    let arrow = if app.show_checks { "▾" } else { "▸" };

    let mut col = column![].spacing(10);
    col = col.push(
        button(
            row![
                text(arrow).size(11).color(ui::GOLD),
                text(" LAUNCH READINESS")
                    .size(13)
                    .font(FONT_CINZEL_BOLD)
                    .color(ui::GOLD),
                space::horizontal(),
                text(glyph).size(12).color(color),
                text(" ").size(12),
                text(summary).size(12).color(ui::MUTED),
            ]
            .width(Length::Fill)
            .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .style(|_t, _s| iced::widget::button::Style {
            background: None,
            text_color: ui::TEXT,
            border: iced::Border::default(),
            shadow: iced::Shadow::default(),
            snap: false,
        })
        .on_press(Message::ToggleChecks),
    );

    if app.show_checks {
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

        let footnote = if failing == 0 {
            muted("Everything checks out — press LAUNCH TOGETHER (or start it from Steam).")
        } else {
            muted(format!(
                "{failing} item(s) need attention — Install fixes all of them automatically."
            ))
        };

        col = col.push(rows);
        col = col.push(ornament());
        col = col.push(footnote);
    }

    Some(card(col))
}

fn activity_log(app: &App) -> Option<Element<'_, Message>> {
    use iced::widget::{container, scrollable};

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
