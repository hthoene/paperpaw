//! A setup step that asks an on/off setting (*Look for updates*, where the
//! device updates its firmware, #132): the switch has the setting's name
//! beside it and sits in its card like a row of the settings pages, not
//! unlabelled at the top of the card.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;
use paperpaw_sdk::firmware::{CHIP_ESP32_C3, Firmware, Plugins};

const GUIDE: &str = "setup.guide";

fn with_firmware(builder: KernelBuilder) -> KernelBuilder {
    builder.firmware(
        Firmware::new("0.6.0", "paperpaw-xteink", CHIP_ESP32_C3).plugins(Plugins::new("")),
    )
}

/// The guide on its page that asks the update step's switch.
fn at_the_step(language: &str) -> App {
    let config = format!("[core]\nlanguage = {language}\n");
    let mut app = App::with_shell(X4, Shared::bare(&config, &[]), with_firmware);
    assert_eq!(app.screen(), GUIDE, "{}", app.described());
    for _ in 0..12 {
        if app.parts().iter().any(|p| p.role == Role::Switch) {
            return app;
        }
        app.run(actions::CONFIRM);
    }
    panic!("no step with a switch:\n{}", app.described());
}

#[test]
fn the_switch_has_its_label_and_sits_in_the_card() {
    for language in ["en", "de"] {
        let mut app = at_the_step(language);
        let parts = app.parts();
        let switch = parts.iter().find(|p| p.role == Role::Switch).unwrap();
        assert_eq!(switch.on, Some(false), "off at first: {}", app.described());
        // The card holds the switch.
        let middle = paperpaw_sdk::Point::new(
            i32::from(switch.area.x + switch.area.width / 2),
            i32::from(switch.area.y + switch.area.height / 2),
        );
        let card = parts
            .iter()
            .find(|p| p.role == Role::Button && p.area.contains(middle))
            .unwrap_or_else(|| panic!("the card holds it:\n{}", app.described()));
        // The name of the setting is drawn in the card, left of the switch
        // (the part list does not carry a row's text): ink there.
        let mut ink = 0;
        for y in card.area.y + 10..card.area.y + card.area.height - 10 {
            for x in card.area.x + 8..switch.area.x.saturating_sub(4) {
                ink += usize::from(app.inked(i32::from(x), i32::from(y)));
            }
        }
        assert!(
            ink > 60,
            "the switch has its label ({ink} inked pixels):\n{}",
            app.described()
        );
        let row_middle = i32::from(switch.area.y) + i32::from(switch.area.height) / 2;
        let card_middle = i32::from(card.area.y) + i32::from(card.area.height) / 2;
        assert!(
            (row_middle - card_middle).abs() <= 4,
            "centred in its card: row {row_middle}, card {card_middle}\n{}",
            app.described()
        );
        assert!(
            i32::from(card.area.x + card.area.width) - i32::from(switch.area.x + switch.area.width)
                <= i32::from(card.area.height) / 2,
            "at the card's right end:\n{}",
            app.described()
        );
    }
}

#[test]
fn the_switch_turns_by_the_button() {
    let mut app = at_the_step("en");
    // The card has the focus; confirming switches it.
    app.run(actions::UP);
    app.run(actions::CONFIRM);
    let parts = app.parts();
    let switch = parts.iter().find(|p| p.role == Role::Switch).unwrap();
    assert_eq!(switch.on, Some(true), "{}", app.described());
}
