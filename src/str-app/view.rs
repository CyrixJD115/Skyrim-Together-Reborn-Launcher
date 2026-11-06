//! Root rendering for the app — modelled on the Skyrim main menu: a white
//! dragon emblem over near-black, gold accents and carved-caps Cinzel
//! headings. The top bar holds the four sections (rendered in the `tab_*`
//! sibling modules) so each screen stays short and focused.

use iced::widget::{button, column, container, progress_bar, row, scrollable, space, svg, text};
use iced::{Element, Length};

use super::tab_client;
use super::tab_info;
use super::tab_server;
use super::tab_settings;
use super::widgets::{link_btn, ornament, secondary_btn, LINK_APIKEY};
use super::{App, Banner, Message, Phase, Tab};
use crate::paths::APP_VERSION;
use crate::ui::{self, FONT_CINZEL, FONT_CINZEL_BOLD};

/// The solid-white Skyrim Together dragon (vector), bundled into the binary.
/// Used for the window icon and the corner watermark.
const LOGO_SVG: &[u8] = include_bytes!("../../assets/logo.svg");

/// Content width shared by the header and every tab.
const CONTENT_WIDTH: f32 = 820.0;

/// How visible the corner dragon is (1.0 = solid white).
const WATERMARK_OPACITY: f32 = 0.15;

// Server tab hidden for now — the dedicated-server flow isn't tested yet.
// The code stays (`tab_server`, `str-server`); re-add (Tab::Server, "SERVER")
// here when it's ready.
const TABS: [(Tab, &str); 3] = [
    (Tab::Client, "CLIENT"),
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

    // The dragon watermark is the *bottom layer* of the stack: the actual
    // background the UI sits on, like the Skyrim main menu — cards and text
    // render over it.
    let main = container(iced::widget::stack![watermark(), root])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(ui::screen);

    main.into()
}

/// The dragon corner art: an oversized "stamp" bleeding off the bottom-right
/// corner of the window, drawn as the bottom layer under the UI. Negative
/// right/bottom padding expands the layout area past the window bounds, so
/// the corner-pinned dragon hangs off the frame; the compositor clips the
/// overflow. (Fixed sizes are clamped by iced's layout limits, so padding is
/// the only way to push a widget past the edge.)
fn watermark() -> Element<'static, Message> {
    // How far the dragon hangs past the 1020×820 max window: the bleed.
    let bleed = iced::Padding { top: 0.0, right: -150.0, bottom: -270.0, left: 0.0 };
    container(dragon())
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Bottom)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(bleed)
        .into()
}

/// The oversized dragon itself. Box matches the logo's viewBox aspect
/// (356.87×520 ≈ 0.686) exactly.
fn dragon() -> Element<'static, Message> {
    svg(svg::Handle::from_memory(LOGO_SVG.to_vec()))
        .width(Length::Fixed(560.0))
        .height(Length::Fixed(817.0))
        .content_fit(iced::ContentFit::Contain)
        .opacity(WATERMARK_OPACITY)
        .into()
}

fn tab_page(app: &App) -> Element<'_, Message> {
    let body = match app.tab {
        Tab::Client => tab_client::client_tab(app),
        Tab::Server => tab_server::server_tab(app),
        Tab::Info => tab_info::info_tab(app),
        Tab::Settings => tab_settings::settings_tab(app),
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
    // Brand: dragon emblem + wordmark + version, above the tab row. The
    // logo box keeps the asset's viewBox aspect (356.87×520 ≈ 0.686), so the
    // dragon renders without distortion.
    let brand = row![
        svg(svg::Handle::from_memory(LOGO_SVG.to_vec()))
            .width(Length::Fixed(44.0))
            .height(Length::Fixed(64.0))
            .content_fit(iced::ContentFit::Contain),
        column![
            text("STR LAUNCHER").size(27).font(FONT_CINZEL_BOLD).color(ui::TEXT),
            text(format!("MULTIPLAYER LAUNCHER · v{APP_VERSION}"))
                .size(12)
                .font(FONT_CINZEL)
                .color(ui::GOLD),
        ]
        .spacing(4),
    ]
    .spacing(16)
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
    .spacing(12)
    .max_width(CONTENT_WIDTH);

    container(inner)
        .padding(iced::Padding { top: 18.0, right: 26.0, bottom: 8.0, left: 26.0 })
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
    button(text(label).size(13).font(FONT_CINZEL_BOLD))
        .padding([8, 18])
        .style(ui::tab_button(tab == active))
        .on_press(Message::TabSelected(tab))
}

// ---------------------------------------------------------------------------
// Shared widgets that need app state
// ---------------------------------------------------------------------------

/// The step/progress block shown while a pipeline is running.
pub fn progress_block(app: &App) -> Element<'_, Message> {
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

/// One offline-archive row: the chosen path plus BROWSE/CLEAR buttons. Used by
/// the Settings tab (STR / Address Library) and the Server tab.
pub fn offline_row<'a>(app: &'a App, which: u8, label: &'a str) -> Element<'a, Message> {
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
