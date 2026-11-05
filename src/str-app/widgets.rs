//! Shared view primitives: cards, ornaments, typography, link targets and
//! buttons used by every tab.

use iced::widget::{button, column, container, row, space, text};
use iced::{color, Element, Length};

use super::Message;
use crate::ui::{self, FONT_CINZEL_BOLD};

pub const LINK_NEXUS: &str = "https://www.nexusmods.com/skyrimspecialedition/mods/69993";
pub const LINK_WIKI: &str =
    "https://wiki.tiltedphoques.com/tilted-online/guides/getting-started";
pub const LINK_PLAYGUIDE: &str =
    "https://wiki.tiltedphoques.com/tilted-online/general-information/playguide";
pub const LINK_APIKEY: &str = "https://www.nexusmods.com/settings/api-keys";

/// Warm gold used for link labels.
const GOLD_LINK: iced::Color = color!(0xa8, 0x8b, 0x5c);

pub fn card<'a>(content: iced::widget::Column<'a, Message>) -> Element<'a, Message> {
    container(content.spacing(10))
        .padding(iced::Padding { top: 16.0, right: 18.0, bottom: 16.0, left: 18.0 })
        .width(Length::Fill)
        .style(ui::card)
        .into()
}

/// A thin gold ornament line, fading at both ends.
pub fn ornament() -> Element<'static, Message> {
    let empty: iced::widget::Column<'static, Message> = column![];
    container(empty)
        .height(1)
        .width(Length::Fill)
        .style(ui::gold_rule())
        .into()
}

pub fn section_title(t: &str) -> Element<'static, Message> {
    row![
        text("◆").size(10).color(ui::GOLD),
        space::horizontal().width(8),
        text(t.to_uppercase()).size(15).font(FONT_CINZEL_BOLD).color(ui::GOLD),
    ]
    .into()
}

pub fn muted(t: impl Into<String>) -> Element<'static, Message> {
    text(t.into()).size(13).color(ui::MUTED).into()
}

pub fn kv(label: &str, value: impl Into<String>) -> Element<'static, Message> {
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

pub fn primary_btn(label: impl Into<String>, msg: Message) -> iced::widget::Button<'static, Message> {
    button(text(label.into()).size(15).font(FONT_CINZEL_BOLD))
        .padding([12, 22])
        .style(ui::primary_button())
        .on_press(msg)
}

pub fn go_btn(label: impl Into<String>, msg: Message) -> iced::widget::Button<'static, Message> {
    button(text(label.into()).size(15).font(FONT_CINZEL_BOLD))
        .padding([12, 22])
        .style(ui::go_button())
        .on_press(msg)
}

pub fn secondary_btn(label: impl Into<String>, msg: Message) -> iced::widget::Button<'static, Message> {
    button(text(label.into()).size(12).font(FONT_CINZEL_BOLD))
        .padding([10, 17])
        .style(ui::secondary_button())
        .on_press(msg)
}

pub fn danger_btn(label: impl Into<String>, msg: Message) -> iced::widget::Button<'static, Message> {
    button(text(label.into()).size(12).font(FONT_CINZEL_BOLD))
        .padding([10, 17])
        .style(ui::danger_button())
        .on_press(msg)
}

pub fn link_btn(label: &str, url: &'static str) -> iced::widget::Button<'static, Message> {
    button(
        text(label.to_string())
            .size(12)
            .font(FONT_CINZEL_BOLD)
            .color(GOLD_LINK),
    )
    .padding([8, 10])
    .style(move |_t, s| iced::widget::button::Style {
        background: None,
        text_color: match s {
            iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed => {
                ui::GOLD_BRIGHT
            }
            _ => GOLD_LINK,
        },
        border: iced::Border::default(),
        shadow: Default::default(),
        snap: false,
    })
    .on_press(Message::OpenLink(url))
}
