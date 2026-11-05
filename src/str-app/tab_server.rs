//! The Server tab: dedicated-server download/launch controls and the
//! STServer.ini settings editor.

use iced::widget::{column, row, space, text, text_input};
use iced::{Element, Length};

use super::widgets::{card, go_btn, muted, ornament, secondary_btn, section_title};
use super::view::{offline_row, progress_block};
use super::{App, Message, OpenTarget, Phase, ServerField};
use crate::server::INI_HINT;
use crate::ui::{self, FONT_CINZEL_BOLD};

pub fn server_tab(app: &App) -> Element<'_, Message> {
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
                    secondary_row_buttons(app),
                ]
                .spacing(8)
                .wrap(),
            );
        }
        None => {
            col = col.push(muted(INI_HINT));
            col = col.push(
                row![secondary_row_buttons(app)].spacing(8),
            );
        }
    }

    card(col)
}

/// The RELOAD FROM FILE / OPEN SERVER FOLDER pair, shared by both editor
/// states.
fn secondary_row_buttons(app: &App) -> iced::widget::Row<'_, Message> {
    row![
        secondary_btn("RELOAD FROM FILE", Message::ServerReloadConfig)
            .on_press_maybe((!app.busy()).then_some(Message::ServerReloadConfig)),
        secondary_btn("OPEN SERVER FOLDER", Message::OpenFolder(OpenTarget::Server))
            .on_press_maybe(app.server_installed.then_some(Message::OpenFolder(OpenTarget::Server))),
    ]
    .spacing(8)
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
