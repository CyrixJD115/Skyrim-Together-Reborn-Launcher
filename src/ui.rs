//! Styling for the Skyrim main-menu look: near-black backdrop, hairline
//! parchment borders, gold accents, carved-caps Cinzel type.

use iced::widget::{button, checkbox, container, pick_list, scrollable};
use iced::{Background, Border, Color, Gradient, Shadow};

// -- Skyrim main-menu palette ------------------------------------------------
/// Near-black backdrop edge (the menu fades darker towards the frame).
pub const BG_DARK: Color = Color::from_rgb8(0x08, 0x08, 0x0a);
/// Backdrop centre — a faint warm ember glow, like the menu's smoke.
pub const BG_WARM: Color = Color::from_rgb8(0x19, 0x15, 0x11);
/// Panel behind grouped content.
pub const CARD: Color = Color::from_rgb8(0x12, 0x11, 0x0e);
/// Hairline borders, barely lighter than the panel.
pub const HAIRLINE: Color = Color::from_rgb8(0x2e, 0x29, 0x20);
pub const INPUT_BG: Color = Color::from_rgb8(0x0d, 0x0c, 0x0a);
/// Parchment body text.
pub const TEXT: Color = Color::from_rgb8(0xec, 0xe6, 0xd8);
/// Dimmed parchment.
pub const MUTED: Color = Color::from_rgb8(0x99, 0x92, 0x7f);
/// Skyrim gold — headings, active borders, progress.
pub const GOLD: Color = Color::from_rgb8(0xb6, 0x94, 0x5a);
pub const GOLD_BRIGHT: Color = Color::from_rgb8(0xd8, 0xb4, 0x74);
pub const SUCCESS: Color = Color::from_rgb8(0x7d, 0x9c, 0x6a);
pub const SUCCESS_BRIGHT: Color = Color::from_rgb8(0x99, 0xb8, 0x86);
pub const DANGER: Color = Color::from_rgb8(0xb0, 0x59, 0x45);
pub const DANGER_BRIGHT: Color = Color::from_rgb8(0xcc, 0x74, 0x5e);

/// Cinzel (Trajan-like carved caps) — registered in `main.rs`.
pub const FONT_CINZEL: iced::Font = iced::Font {
    family: iced::font::Family::Name("Cinzel"),
    weight: iced::font::Weight::Normal,
    stretch: iced::font::Stretch::Normal,
    style: iced::font::Style::Normal,
};
/// Cinzel Bold — for the big title.
pub const FONT_CINZEL_BOLD: iced::Font = iced::Font {
    family: iced::font::Family::Name("Cinzel"),
    weight: iced::font::Weight::Bold,
    stretch: iced::font::Stretch::Normal,
    style: iced::font::Style::Normal,
};

/// The near-black menu backdrop: dark frame with a faint warm centre glow,
/// like the Skyrim main menu behind the logo.
pub fn screen(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Gradient(
            Gradient::Linear(
                iced::gradient::Linear::new(iced::Radians::from(std::f32::consts::FRAC_PI_2))
                    .add_stop(0.0, BG_DARK)
                    .add_stop(0.55, BG_WARM)
                    .add_stop(1.0, BG_DARK),
            ),
        )),
        text_color: Some(TEXT),
        ..Default::default()
    }
}

fn panel_border() -> Border {
    Border {
        radius: 2.0.into(),
        width: 1.0,
        color: HAIRLINE,
    }
}

pub fn card(theme: &iced::Theme) -> container::Style {
    let _ = theme;
    container::Style {
        background: Some(Background::Color(CARD)),
        text_color: Some(TEXT),
        border: panel_border(),
        shadow: Shadow::default(),
        snap: false,
    }
}

pub fn card_flat(theme: &iced::Theme) -> container::Style {
    let _ = theme;
    container::Style {
        background: Some(Background::Color(Color::from_rgb8(0x0e, 0x0d, 0x0b))),
        text_color: Some(TEXT),
        border: Border {
            radius: 2.0.into(),
            width: 1.0,
            color: Color::from_rgb8(0x24, 0x21, 0x19),
        },
        snap: false,
        ..Default::default()
    }
}

fn button_style(
    base_border: Color,
    hover_border: Color,
    base_text: Color,
    hover_text: Color,
) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let (bg, border_c, text_color) = match status {
            button::Status::Hovered => (Color::from_rgba(0.72, 0.58, 0.35, 0.08), hover_border, hover_text),
            button::Status::Pressed => (Color::from_rgba(0.72, 0.58, 0.35, 0.16), hover_border, hover_text),
            button::Status::Disabled => (Color::from_rgba(0.0, 0.0, 0.0, 0.0), HAIRLINE, Color::from_rgb8(0x5a, 0x54, 0x48)),
            _ => (Color::from_rgba(0.0, 0.0, 0.0, 0.0), base_border, base_text),
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color,
            border: Border {
                radius: 2.0.into(),
                width: 1.0,
                color: border_c,
            },
            shadow: Shadow::default(),
            snap: false,
        }
    }
}

/// Primary action — gold border like the menu's selected entry.
pub fn primary_button() -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    button_style(GOLD, GOLD_BRIGHT, GOLD_BRIGHT, Color::from_rgb8(0xf0, 0xd9, 0xac))
}

/// Go/launch action — success green kept subtle and desaturated.
pub fn go_button() -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    button_style(SUCCESS, SUCCESS_BRIGHT, SUCCESS_BRIGHT, Color::from_rgb8(0xc2, 0xdb, 0xb2))
}

/// Secondary action — hairline parchment.
pub fn secondary_button() -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    button_style(
        Color::from_rgb8(0x4a, 0x43, 0x35),
        GOLD,
        TEXT,
        GOLD_BRIGHT,
    )
}

/// Destructive action — dark iron red.
pub fn danger_button() -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    button_style(
        Color::from_rgb8(0x4a, 0x2c, 0x24),
        DANGER,
        Color::from_rgb8(0xd0, 0xa8, 0x9c),
        DANGER_BRIGHT,
    )
}

/// Top-bar tab — a quiet ghost button that turns gold when active.
pub fn tab_button(active: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(
            status,
            button::Status::Hovered | button::Status::Pressed
        );
        let (background, border, text_color) = if active {
            (
                Color::from_rgba(0.72, 0.58, 0.35, 0.12),
                GOLD,
                GOLD_BRIGHT,
            )
        } else if hovered {
            (
                Color::from_rgba(0.72, 0.58, 0.35, 0.06),
                HAIRLINE,
                GOLD_BRIGHT,
            )
        } else {
            (Color::TRANSPARENT, Color::TRANSPARENT, MUTED)
        };

        button::Style {
            background: Some(Background::Color(background)),
            text_color,
            border: Border {
                radius: 2.0.into(),
                width: 1.0,
                color: border,
            },
            shadow: Shadow::default(),
            snap: false,
        }
    }
}

pub fn text_input(
    theme: &iced::Theme,
    status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    let _ = theme;
    let border_color = match status {
        iced::widget::text_input::Status::Focused { .. } => GOLD,
        iced::widget::text_input::Status::Disabled => HAIRLINE,
        _ => HAIRLINE,
    };
    iced::widget::text_input::Style {
        background: Background::Color(INPUT_BG),
        border: Border {
            radius: 2.0.into(),
            width: 1.0,
            color: border_color,
        },
        icon: MUTED,
        placeholder: Color::from_rgb8(0x6a, 0x63, 0x54),
        value: TEXT,
        selection: Color::from_rgba(0.71, 0.58, 0.35, 0.35),
    }
}

pub fn checkbox_style(
    _theme: &iced::Theme,
    status: checkbox::Status,
) -> checkbox::Style {
    let (checked, hovered, disabled) = match status {
        checkbox::Status::Active { is_checked } => (is_checked, false, false),
        checkbox::Status::Hovered { is_checked } => (is_checked, true, false),
        checkbox::Status::Disabled { is_checked } => (is_checked, false, true),
    };

    let (background, icon_color, border_color, text_color) = if disabled {
        (
            INPUT_BG,
            MUTED,
            HAIRLINE,
            Color::from_rgb8(0x5a, 0x54, 0x48),
        )
    } else if checked {
        (
            if hovered { GOLD_BRIGHT } else { GOLD },
            BG_DARK,
            GOLD_BRIGHT,
            TEXT,
        )
    } else {
        (
            INPUT_BG,
            GOLD,
            if hovered { GOLD } else { HAIRLINE },
            TEXT,
        )
    };

    checkbox::Style {
        background: Background::Color(background),
        icon_color,
        border: Border {
            radius: 2.0.into(),
            width: 1.0,
            color: border_color,
        },
        text_color: Some(text_color),
    }
}

pub fn pick_list_style(
    _theme: &iced::Theme,
    status: pick_list::Status,
) -> pick_list::Style {
    let hovered = matches!(
        status,
        pick_list::Status::Hovered | pick_list::Status::Opened { .. }
    );

    pick_list::Style {
        text_color: TEXT,
        placeholder_color: MUTED,
        handle_color: if hovered { GOLD_BRIGHT } else { GOLD },
        background: Background::Color(INPUT_BG),
        border: Border {
            radius: 2.0.into(),
            width: 1.0,
            color: if hovered { GOLD } else { HAIRLINE },
        },
    }
}

pub fn scrollable_style(_theme: &iced::Theme, _status: scrollable::Status) -> scrollable::Style {
    let rail = scrollable::Rail {
        background: None,
        border: Border::default(),
        scroller: scrollable::Scroller {
            background: Background::Color(Color::from_rgb8(0x38, 0x31, 0x25)),
            border: Border {
                radius: 3.0.into(),
                width: 0.0,
                color: Color::TRANSPARENT,
            },
        },
    };

    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: Background::Color(CARD),
            border: Border {
                radius: 2.0.into(),
                width: 1.0,
                color: HAIRLINE,
            },
            shadow: Shadow::default(),
            icon: MUTED,
        },
    }
}

pub fn progress_bar(theme: &iced::Theme) -> iced::widget::progress_bar::Style {
    let _ = theme;
    iced::widget::progress_bar::Style {
        background: Background::Color(Color::from_rgb8(0x1c, 0x19, 0x14)),
        bar: Background::Color(GOLD),
        border: Border {
            radius: 1.0.into(),
            width: 1.0,
            color: HAIRLINE,
        },
    }
}

/// A thin gold-fading rule, like the menu's ornament separators.
pub fn gold_rule() -> impl Fn(&iced::Theme) -> container::Style {
    |_theme| container::Style {
        background: Some(Background::Gradient(Gradient::Linear(
            iced::gradient::Linear::new(iced::Radians::from(0.0))
                .add_stop(0.0, Color::from_rgba(0.71, 0.58, 0.35, 0.0))
                .add_stop(0.5, Color::from_rgba(0.71, 0.58, 0.35, 0.75))
                .add_stop(1.0, Color::from_rgba(0.71, 0.58, 0.35, 0.0)),
        ))),
        ..Default::default()
    }
}
