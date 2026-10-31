//! The colour roles are open: a role a theme does not set follows its
//! fallback, whatever the number, and the names a theme writes stay.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod support;

use paperpaw_sdk::Color;
use paperpaw_sdk::ui::{Ink, Palette};

/// The names of the first 14 roles are what themes write: they never
/// change.
#[test]
fn the_names_themes_write_are_pinned() {
    let names = [
        "text",
        "paper",
        "note",
        "detail",
        "muted",
        "faint",
        "disabled",
        "rule",
        "line",
        "line-faint",
        "on-focus-muted",
        "on-dark-muted",
        "card",
        "section",
        "accent",
        "on-accent",
        "link",
        "selection",
        "warning",
    ];
    assert_eq!(Ink::ALL.len(), names.len());
    for (at, (ink, name)) in Ink::ALL.iter().zip(names).enumerate() {
        assert_eq!(ink.name(), name);
        assert_eq!(usize::from(ink.value()), at, "{name}");
        assert_eq!(Ink::from_value(ink.value()), *ink);
    }
    assert_eq!(Ink::from_value(200).name(), "");
}

#[test]
fn debug_prints_the_role() {
    assert_eq!(format!("{:?}", Ink::LINE_FAINT), "Ink::LINE_FAINT");
    assert_eq!(format!("{:?}", Ink::WARNING), "Ink::WARNING");
    assert_eq!(format!("{:?}", Ink::from_value(77)), "Ink(77)");
}

/// Every fallback ends at one of the first 14 roles, which have a colour
/// of their own, in a step or two, for every number.
#[test]
fn every_fallback_chain_ends_at_one_of_the_first_14() {
    for value in 0..=255u8 {
        let mut ink = Ink::from_value(value);
        let mut steps = 0;
        while ink.value() >= 14 {
            ink = ink.fallback();
            steps += 1;
            assert!(steps <= 4, "role {value} does not end");
        }
        assert_eq!(ink.fallback(), ink);
    }
    assert_eq!(Ink::ACCENT.fallback(), Ink::TEXT);
    assert_eq!(Ink::ON_ACCENT.fallback(), Ink::PAPER);
    assert_eq!(Ink::LINK.fallback(), Ink::TEXT);
    assert_eq!(Ink::SELECTION.fallback(), Ink::CARD);
    assert_eq!(Ink::WARNING.fallback(), Ink::TEXT);
    assert_eq!(Ink::from_value(200).fallback(), Ink::TEXT);
}

/// A theme made for 14 roles gives the later roles its colours.
#[test]
fn a_role_the_theme_leaves_out_follows_its_fallback() {
    let palette = Palette::parse("text=40 paper=250 card=220");
    assert_eq!(palette.color(Ink::LINK), Color::gray(40));
    assert_eq!(palette.color(Ink::ACCENT), Color::gray(40));
    assert_eq!(palette.color(Ink::ON_ACCENT), Color::gray(250));
    assert_eq!(palette.color(Ink::SELECTION), Color::gray(220));
    assert_eq!(palette.color(Ink::from_value(25)), Color::gray(40));
    assert_eq!(palette.color(Ink::from_value(255)), Color::gray(40));
    // What the theme sets is its own.
    let own = Palette::parse("text=40 link=#2a6f97");
    assert_eq!(own.color(Ink::LINK), Color::rgb(0x2a, 0x6f, 0x97));
    assert_eq!(own.color(Ink::TEXT), Color::gray(40));
    // The defaults draw as they did: nothing looks different.
    for ink in Ink::ALL {
        let fallback = ink.fallback();
        assert_eq!(
            Palette::DEFAULT.color(*ink),
            Palette::DEFAULT.color(fallback),
            "{ink:?}"
        );
    }
}

/// `#rrggbb` is read by this release; a reader that knows grays alone
/// skips the pair (as it does any value it cannot read).
#[test]
fn colours_are_read_and_bad_ones_skipped() {
    let palette = Palette::parse("accent=#FFaa00 link=#12345 warning=#gg0000 note=#000000");
    assert_eq!(palette.color(Ink::ACCENT), Color::rgb(255, 170, 0));
    assert_eq!(palette.color(Ink::LINK), Palette::DEFAULT.color(Ink::TEXT));
    assert_eq!(
        palette.color(Ink::WARNING),
        Palette::DEFAULT.color(Ink::TEXT)
    );
    assert_eq!(palette.color(Ink::NOTE), Color::BLACK);
    // The last of two entries wins.
    let twice = Palette::parse("muted=10 muted=200");
    assert_eq!(twice.color(Ink::MUTED), Color::gray(200));
}

/// Strong contrast draws gray text black, and a role that reads a text
/// role's colour is text too; one that reads text or paper is not.
#[test]
fn strong_contrast_follows_the_fallback() {
    use paperpaw_sdk::ui::UiStyle;
    use support::{Device, Harness};
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env)
        .with_palette(Palette::parse("muted=90 link=#2a6f97"))
        .with_strong(true);
    assert_eq!(style.ink(Ink::MUTED), Color::BLACK);
    assert_eq!(style.ink(Ink::FAINT), Color::BLACK);
    assert_eq!(style.ink(Ink::LINK), Color::rgb(0x2a, 0x6f, 0x97));
    assert_eq!(style.ink(Ink::PAPER), Color::WHITE);
    assert_eq!(style.ink(Ink::from_value(99)), Color::BLACK);
}
