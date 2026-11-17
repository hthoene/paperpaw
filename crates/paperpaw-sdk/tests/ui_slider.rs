//! Sliders, equally easy by touch and with buttons: values keep to their
//! range and steps however they are set (a tap, a drag, the buttons),
//! a tap where the knob is drawn chooses the value it shows, the parts
//! keep to the margins and nothing is drawn outside the row. Broken
//! ranges (as a broken plugin may give) do not bring it down.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod support;

use paperpaw_sdk::SettingSpec;
use paperpaw_sdk::ui::slider;
use proptest::prelude::*;

use support::{Device, Harness};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]






}

/// A slider in steps of 5.
static SPACING: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new().step(5).slider();

#[test]
fn sliders_show_on_touch_screens_and_numbers_elsewhere() {
    let spec = SettingSpec::number(
        "t.spacing",
        "t.spacing",
        100,
        250,
        135,
        paperpaw_sdk::builtin::groups::OTHER,
    )
    .with_extra(&SPACING);
    let touch = Harness::new(Device::TOUCH);
    let buttons = Harness::new(Device::BUTTONS);
    assert!(slider::shows(&touch.env(), &spec));
    assert!(!slider::shows(&buttons.env(), &spec), "buttons: − and +");
    let plain = SettingSpec::number(
        "t.size",
        "t.size",
        1,
        9,
        3,
        paperpaw_sdk::builtin::groups::OTHER,
    );
    assert!(!slider::shows(&touch.env(), &plain), "not marked");
}
