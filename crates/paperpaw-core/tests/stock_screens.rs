//! The kernel's stock screens (moved from the SDK, #244 D6b), by touch
//! and by buttons alike: the screen choosing a status line's items
//! ([`ItemsScreen`]) shows, hides and reorders them, writing the setting
//! the same whether a finger or a button did it. (The choice is the dialog
//! plugin's since #293: `paperpaw-app`'s `tests/dialogs.rs`.)

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

#[path = "../../paperpaw-sdk/tests/support/mod.rs"]
mod support;

use std::borrow::Cow;

use paperpaw_core::Layer;
use paperpaw_core::stock::items::ItemsScreen;
use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::ui::reorder::Reorder;
use paperpaw_sdk::{
    Command, Cx, Handled, Id, Output, Params, PointerPhase, Rect, SettingSpec, SettingValue,
    StatusItemSpec, StatusValue,
};

use support::{Device, Harness, describe, drawn_outside, middle, pointer, tap};

fn run<R>(h: &Harness, f: impl FnOnce(&mut Cx<'_, '_>) -> R) -> (R, Vec<Command>) {
    let env = h.env();
    let mut output = Output::default();
    let result = {
        let mut cx = Cx::new(&env, &mut output);
        f(&mut cx)
    };
    (result, output.commands)
}

// ---- Choosing a status line's items -------------------------------------

const STATUS: Setting<paperpaw_sdk::Text> = Setting::new("t.status");

fn status_harness(device: Device, value: &str) -> Harness {
    let mut h = Harness::new(device);
    for (id, title) in [
        ("t.time", "t.time"),
        ("t.page", "t.page"),
        ("t.battery", "t.battery"),
    ] {
        h.status_items
            .add(StatusItemSpec::new(Id::new_static(id), title, |_, _| {
                Some(StatusValue::Text("x".into()))
            }));
    }
    h.settings
        .define(SettingSpec::new(
            paperpaw_sdk::Id::from(STATUS),
            "t.status",
            paperpaw_sdk::SettingKind::List,
            paperpaw_sdk::SettingValue::Text(Cow::Owned(value.to_string())),
            paperpaw_sdk::builtin::groups::OTHER,
        ))
        .unwrap();
    h
}

fn items_screen(h: &Harness) -> ItemsScreen {
    let params = Params::new().with(params::ID, "t.status");
    ItemsScreen::open(&h.env(), &params)
}

/// The value the screen wrote for the setting.
fn written(commands: &[Command]) -> Option<String> {
    commands.iter().find_map(|c| match c {
        Command::SetSetting(id, SettingValue::Text(v)) if id.as_str() == "t.status" => {
            Some(v.to_string())
        }
        _ => None,
    })
}

/// The rows of the items screen, as drawn: shown items first, in their
/// order, then the others.
fn item_rows(h: &Harness, screen: &mut ItemsScreen) -> Vec<Rect> {
    let (described, mut frame) = describe(480, 800, |c| screen.draw(c, &h.env()));
    assert_eq!(drawn_outside(&mut frame, Rect::new(0, 0, 480, 800)), None);
    described
        .iter()
        .filter(|d| d.role == paperpaw_sdk::semantics::Role::Row)
        .map(|d| d.area)
        .collect()
}

#[test]
fn items_are_shown_and_hidden_alike_by_tap_and_by_confirm() {
    // "t.page, t.time" shown: rows page, time, then battery.
    for (row, expected) in [
        (0, "t.time"),
        (1, "t.page"),
        (2, "t.page, t.time, t.battery"),
    ] {
        let mut by_tap = None;
        let mut by_button = None;
        for device in [Device::TOUCH, Device::BUTTONS] {
            let h = status_harness(device, "t.page, t.time");
            let mut screen = items_screen(&h);
            if device.touch {
                let rows = item_rows(&h, &mut screen);
                assert_eq!(rows.len(), 3);
                // On the row, away from the grip.
                let at = rows[row];
                let (_, commands) = run(&h, |cx| {
                    screen.on_gesture(tap(i32::from(at.x) + 20, middle(at).y), cx)
                });
                by_tap = written(&commands);
            } else {
                for _ in 0..row {
                    run(&h, |cx| screen.on_action(&actions::DOWN, cx));
                }
                let (_, commands) = run(&h, |cx| screen.on_action(&actions::CONFIRM, cx));
                by_button = written(&commands);
            }
        }
        assert_eq!(by_tap.as_deref(), Some(expected), "row {row}");
        assert_eq!(by_button, by_tap, "row {row}");
    }
}

#[test]
fn shown_items_are_reordered_by_their_grip() {
    let h = status_harness(Device::TOUCH, "t.page, t.time, t.battery");
    let mut screen = items_screen(&h);
    let rows = item_rows(&h, &mut screen);
    let grip = middle(Reorder::grip(rows[0], &UiStyle::PLAIN));
    let to = middle(rows[2]);
    let (_, commands) = run(&h, |cx| {
        assert_eq!(
            screen.on_pointer(&pointer(PointerPhase::Down, grip.x, grip.y), cx),
            Handled::Yes
        );
        screen.on_pointer(&pointer(PointerPhase::Move, grip.x, to.y), cx);
        screen.on_pointer(&pointer(PointerPhase::Up, grip.x, to.y), cx)
    });
    assert_eq!(
        written(&commands).as_deref(),
        Some("t.time, t.battery, t.page")
    );
    // Items not shown have no grip: a finger there is a tap's.
    let h = status_harness(Device::TOUCH, "t.page");
    let mut screen = items_screen(&h);
    let rows = item_rows(&h, &mut screen);
    let grip = middle(Reorder::grip(rows[1], &UiStyle::PLAIN));
    let (handled, _) = run(&h, |cx| {
        screen.on_pointer(&pointer(PointerPhase::Down, grip.x, grip.y), cx)
    });
    assert_eq!(handled, Handled::No);
}

/// Items of a plugin that is not there now (switched off, being
/// updated) are part of the setting: changing the others keeps them, so
/// they are back in their place when the plugin is.
#[test]
fn items_of_a_plugin_not_there_now_are_kept() {
    let h = status_harness(Device::BUTTONS, "t.page, gone.clock, t.time");
    let mut screen = items_screen(&h);
    // Hide the time (the second row).
    run(&h, |cx| screen.on_action(&actions::DOWN, cx));
    let (_, commands) = run(&h, |cx| screen.on_action(&actions::CONFIRM, cx));
    let value = written(&commands).unwrap();
    assert!(value.contains("gone.clock"), "{value}");
}

/// With buttons alone (the X4), shown items are moved as by their grip:
/// *Menu* takes the focused one along, the direction buttons move it a
/// place, *Confirm* puts it down; then the buttons move the focus again.
#[test]
fn shown_items_are_reordered_by_buttons_alone() {
    let h = status_harness(Device::BUTTONS, "t.page, t.time, t.battery");
    let mut screen = items_screen(&h);
    let env = h.env();
    assert_eq!(
        screen.hint(&actions::MENU, &env),
        paperpaw_sdk::Hint::Text("core.move".into())
    );
    run(&h, |cx| screen.on_action(&actions::MENU, cx));
    assert_eq!(
        screen.hint(&actions::CONFIRM, &env),
        paperpaw_sdk::Hint::Text("core.grid.done".into())
    );
    let (_, commands) = run(&h, |cx| screen.on_action(&actions::DOWN, cx));
    assert_eq!(
        written(&commands).as_deref(),
        Some("t.time, t.page, t.battery")
    );
    // Back puts it down too and does not leave the screen.
    let (handled, commands) = run(&h, |cx| screen.on_action(&actions::BACK, cx));
    assert_eq!(handled, Handled::Yes);
    assert!(!commands.contains(&Command::Back));
    // The buttons move the focus again: Confirm hides the next item.
    run(&h, |cx| screen.on_action(&actions::DOWN, cx));
    let (_, commands) = run(&h, |cx| screen.on_action(&actions::CONFIRM, cx));
    assert!(written(&commands).is_some());
}

/// Menu takes an item along only where there is another shown item to
/// move it past; its hint says so only then. While it is taken, a frame
/// in the focus's ink inside the focused row shows it, not hidden in the
/// focus.
#[test]
fn an_item_is_taken_along_only_past_another_and_shows_it() {
    let h = status_harness(Device::BUTTONS, "t.page");
    let mut screen = items_screen(&h);
    let env = h.env();
    assert_eq!(
        screen.hint(&actions::MENU, &env),
        paperpaw_sdk::Hint::Default
    );
    let (handled, _) = run(&h, |cx| screen.on_action(&actions::MENU, cx));
    assert_eq!(handled, Handled::No, "left to the kernel's menu");

    let h = status_harness(Device::BUTTONS, "t.page, t.time");
    let mut screen = items_screen(&h);
    let env = h.env();
    let style = UiStyle::new(&env);
    run(&h, |cx| screen.on_action(&actions::MENU, cx));
    let (described, frame) = describe(480, 800, |c| screen.draw(c, &env));
    let row = described
        .iter()
        .find(|d| d.role == paperpaw_sdk::semantics::Role::Row && d.focused)
        .map(|d| d.area)
        .expect("the focused row");
    // The frame's top, a little inside the row, in the focus's ink.
    let y = row.y + (u32::from(style.line()) * 3 + u32::from(style.line())) as u16;
    let x = row.x + row.width / 2;
    assert_eq!(frame.view().pixel(x, y), style.focus_ink(true));
}

/// On a touch screen the focus shows only once a button moved it: a
/// first *Confirm* shows it instead of choosing a row nobody saw.
#[test]
fn a_first_confirm_on_a_touch_screen_shows_the_focus() {
    let h = status_harness(Device::TOUCH, "t.page");
    let mut screen = items_screen(&h);
    let (_, commands) = run(&h, |cx| screen.on_action(&actions::CONFIRM, cx));
    assert_eq!(written(&commands), None);
    let (_, commands) = run(&h, |cx| screen.on_action(&actions::CONFIRM, cx));
    assert_eq!(written(&commands).as_deref(), Some(""));
}

#[test]
fn a_status_line_setting_without_an_id_writes_nothing() {
    let h = status_harness(Device::BUTTONS, "t.page");
    let mut screen = ItemsScreen::open(&h.env(), &Params::new().with(params::ID, "Not An Id"));
    let (handled, commands) = run(&h, |cx| screen.on_action(&actions::CONFIRM, cx));
    assert_eq!(handled, Handled::Yes);
    assert_eq!(written(&commands), None);
    let style = UiStyle::new(&h.env());
    assert!(u32::from(style.row()) > 0);
}

/// A line kept under an id it had before shows as chosen, by its own; a
/// press keeps it once, under its own id, and off it goes.
#[test]
fn a_line_kept_by_its_old_id_is_chosen_once_and_kept_by_its_own() {
    let mut h = status_harness(Device::BUTTONS, "t.old-line, t.line");
    for id in ["t.line", "t.other-line"] {
        h.status_items.add_line(
            paperpaw_sdk::StatusLineSpec::new(Id::new_static(id), id, |_, _, _| {}).also(
                Id::new_static(if id == "t.line" {
                    "t.old-line"
                } else {
                    "t.older"
                }),
            ),
        );
    }
    let params = Params::new()
        .with(params::ID, "t.status")
        .with(params::MODE, paperpaw_core::stock::items::LINES);
    let mut screen = ItemsScreen::open(&h.env(), &params);
    // The first row is the line chosen, once: confirm switches it off.
    let (_, commands) = run(&h, |cx| screen.on_action(&actions::CONFIRM, cx));
    assert_eq!(written(&commands).as_deref(), Some(""));
    // Down to the other and on: the first kept by its own id, once.
    let mut h = h;
    h.settings
        .set("t.status", SettingValue::Text("t.old-line".into()))
        .unwrap();
    let mut screen = ItemsScreen::open(&h.env(), &params);
    run(&h, |cx| screen.on_action(&actions::DOWN, cx));
    let (_, commands) = run(&h, |cx| screen.on_action(&actions::CONFIRM, cx));
    assert_eq!(written(&commands).as_deref(), Some("t.line, t.other-line"));
}
