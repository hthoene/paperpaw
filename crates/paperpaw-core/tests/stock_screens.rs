//! The kernel's stock screens (moved from the SDK, #244 D6b), by touch
//! and by buttons alike: the
//! choice overlay ([`Choice`]) picks the option tapped or confirmed and
//! says so with its reply, and the screen choosing a status line's items
//! ([`ItemsScreen`]) shows, hides and reorders them, writing the setting
//! the same whether a finger or a button did it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

#[path = "../../paperpaw-sdk/tests/support/mod.rs"]
mod support;

use std::borrow::Cow;

use paperpaw_core::Layer;
use paperpaw_core::stock::choice::Choice;
use paperpaw_core::stock::items::ItemsScreen;
use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::event::{Event, EventData};
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::ui::choice::Picked;
use paperpaw_sdk::ui::reorder::Reorder;
use paperpaw_sdk::{
    Command, Cx, Handled, Id, Output, Params, PointerPhase, Rect, SettingSpec, SettingValue,
    StatusItemSpec, StatusValue,
};

use support::{Device, Harness, describe, drawn_outside, middle, pointer, tap};

const CHOSEN: Event<Picked> = Event::new("t.chosen");

/// The parameters `ask::choice` gives the screen of slot `core.choice`.
fn opened(options: &[&str]) -> Params {
    let Command::OpenSlot(slot, params) = paperpaw_sdk::ask::choice("Delete “a.epub”?", &CHOSEN)
        .about("books/a.epub")
        .options("t.option.", options)
        .command()
    else {
        panic!("not a slot");
    };
    assert_eq!(slot, paperpaw_sdk::builtin::slots::CHOICE);
    params
}

fn run<R>(h: &Harness, f: impl FnOnce(&mut Cx<'_, '_>) -> R) -> (R, Vec<Command>) {
    let env = h.env();
    let mut output = Output::default();
    let result = {
        let mut cx = Cx::new(&env, &mut output);
        f(&mut cx)
    };
    (result, output.commands)
}

/// The option a reply among `commands` picked, after going back.
fn picked(commands: &[Command]) -> Option<String> {
    match commands {
        [Command::Back, Command::Emit(event, params)] => {
            assert_eq!(event.as_str(), CHOSEN.as_str());
            let picked = Picked::from_params(params).unwrap();
            assert_eq!(picked.path, "books/a.epub");
            Some(picked.option)
        }
        [Command::Back] | [] => None,
        other => panic!("{other:?}"),
    }
}

// ---- The choice overlay -------------------------------------------------

#[test]
fn each_option_is_picked_alike_by_tap_and_by_confirm() {
    for device in [Device::TOUCH, Device::BUTTONS] {
        let h = Harness::new(device);
        let options = ["open", "rename", "delete", "cancel"];
        let mut screen = Choice::new(&h.env(), &opened(&options));
        let region = screen.region(&h.env()).unwrap();
        assert!(screen.dims_below());
        assert_eq!(region.bottom(), 800, "at the bottom");
        // Drawn inside its region, one row for each option.
        let (described, mut frame) = describe(480, 800, |c| screen.draw(c, &h.env()));
        assert_eq!(drawn_outside(&mut frame, region), None);
        let rows: Vec<Rect> = described
            .iter()
            .filter(|d| d.role == paperpaw_sdk::semantics::Role::Row)
            .map(|d| d.area)
            .collect();
        assert_eq!(rows.len(), options.len());
        // Tapped.
        let mut by_tap = Vec::new();
        for row in &rows {
            let m = middle(*row);
            let mut screen = Choice::new(&h.env(), &opened(&options));
            let (handled, commands) = run(&h, |cx| screen.on_gesture(tap(m.x, m.y), cx));
            assert_eq!(handled, Handled::Yes);
            by_tap.push(picked(&commands).unwrap());
        }
        assert_eq!(by_tap, options);
        // With the buttons: down to each, then confirm.
        let mut by_buttons = Vec::new();
        for n in 0..options.len() {
            let mut screen = Choice::new(&h.env(), &opened(&options));
            // On touch screens the first press shows the focus.
            let presses = n + usize::from(device.touch);
            for _ in 0..presses {
                run(&h, |cx| screen.on_action(&actions::DOWN, cx));
            }
            let (_, commands) = run(&h, |cx| screen.on_action(&actions::CONFIRM, cx));
            by_buttons.push(picked(&commands).unwrap());
        }
        assert_eq!(by_buttons, options);
    }
}

#[test]
fn the_focus_goes_round_and_back_picks_nothing() {
    let h = Harness::new(Device::BUTTONS);
    let options = ["a", "b", "c"];
    let mut screen = Choice::new(&h.env(), &opened(&options));
    // Up from the first: the last.
    run(&h, |cx| screen.on_action(&actions::UP, cx));
    let (_, commands) = run(&h, |cx| screen.on_action(&actions::CONFIRM, cx));
    assert_eq!(picked(&commands).as_deref(), Some("c"));
    // The page buttons step too (a short list is one page).
    let mut screen = Choice::new(&h.env(), &opened(&options));
    run(&h, |cx| screen.on_action(&actions::NEXT_PAGE, cx));
    let (_, commands) = run(&h, |cx| screen.on_action(&actions::CONFIRM, cx));
    assert_eq!(picked(&commands).as_deref(), Some("b"));
    // Back is left to the kernel: it closes the overlay, nothing picked.
    let (handled, commands) = run(&h, |cx| screen.on_action(&actions::BACK, cx));
    assert_eq!(handled, Handled::No);
    assert!(commands.is_empty());
}

#[test]
fn broken_parameters_make_an_empty_choice_that_picks_nothing() {
    let h = Harness::new(Device::TOUCH);
    let params = Params::new()
        .with("options", ",,,")
        .with(params::REPLY, "Not An Id");
    let mut screen = Choice::new(&h.env(), &params);
    let region = screen.region(&h.env()).unwrap();
    assert!(region.bottom() <= 800);
    let (_, mut frame) = describe(480, 800, |c| screen.draw(c, &h.env()));
    assert_eq!(drawn_outside(&mut frame, region), None);
    let (_, commands) = run(&h, |cx| screen.on_action(&actions::CONFIRM, cx));
    assert_eq!(commands, [Command::Back], "closed, nothing to say");
    let m = middle(region);
    let (_, commands) = run(&h, |cx| screen.on_gesture(tap(m.x, m.y), cx));
    assert!(commands.is_empty());
    // No parameters at all.
    let screen = Choice::new(&h.env(), &Params::new());
    assert!(screen.region(&h.env()).unwrap().bottom() <= 800);
}

#[test]
fn a_long_title_takes_up_to_three_lines_and_the_options_stay_below_it() {
    let h = Harness::new(Device::TOUCH);
    let title = "Delete all the books of the series “The Long Name of a Series That Goes On and On” and their notes, highlights and bookmarks from this device?";
    let params = opened(&["delete", "cancel"]).with(params::TITLE, title);
    let screen = Choice::new(&h.env(), &params);
    let region = screen.region(&h.env()).unwrap();
    let mut screen = screen;
    let (described, mut frame) = describe(480, 800, |c| screen.draw(c, &h.env()));
    assert_eq!(drawn_outside(&mut frame, region), None);
    let rows: Vec<Rect> = described
        .iter()
        .filter(|d| d.role == paperpaw_sdk::semantics::Role::Row)
        .map(|d| d.area)
        .collect();
    let texts: Vec<Rect> = described
        .iter()
        .filter(|d| d.role == paperpaw_sdk::semantics::Role::Text && d.area.y < rows[0].y)
        .map(|d| d.area)
        .collect();
    assert!((1..=3).contains(&texts.len()), "{} lines", texts.len());
    assert!(texts.iter().all(|t| t.bottom() <= u32::from(rows[0].y)));
}

/// More options than the screen holds (a plugin offering many): every
/// option is still to be reached, on the screen.
#[test]
fn more_options_than_fit_stay_on_the_screen() {
    let h = Harness::new(Device::TOUCH);
    let options: Vec<String> = (0..30).map(|n| format!("option{n}")).collect();
    let options: Vec<&str> = options.iter().map(String::as_str).collect();
    let mut screen = Choice::new(&h.env(), &opened(&options));
    let (described, _) = describe(480, 800, |c| screen.draw(c, &h.env()));
    let rows: Vec<Rect> = described
        .iter()
        .filter(|d| d.role == paperpaw_sdk::semantics::Role::Row)
        .map(|d| d.area)
        .collect();
    assert!(!rows.is_empty());
    assert!(
        rows.iter().all(|r| r.bottom() <= 800),
        "rows below the screen: {:?}",
        rows.last()
    );
}

/// Thirty options on pages: by touch, the pager's right half turns to
/// the next page and a tap picks option 29 on the last; by buttons the
/// page buttons step a row (the X4's buttons beside the screen) and reach
/// it too; a swipe on a touch screen (a page button, the focus hidden)
/// turns a page.
#[test]
fn every_option_on_pages_is_reached_by_touch_and_by_buttons() {
    let names: Vec<String> = (0..30).map(|n| format!("o{n}")).collect();
    let options: Vec<&str> = names.iter().map(String::as_str).collect();
    let rows_of = |h: &Harness, screen: &mut Choice| -> (Vec<Rect>, Option<Rect>) {
        let (described, _) = describe(480, 800, |c| screen.draw(c, &h.env()));
        let rows = described
            .iter()
            .filter(|d| d.role == paperpaw_sdk::semantics::Role::Row)
            .map(|d| d.area)
            .collect();
        let pager = described
            .iter()
            .find(|d| d.label.contains(" / "))
            .map(|d| d.area);
        (rows, pager)
    };
    // By touch.
    let h = Harness::new(Device::TOUCH);
    let mut screen = Choice::new(&h.env(), &opened(&options));
    let (rows, pager) = rows_of(&h, &mut screen);
    let per_page = rows.len();
    assert!(per_page < 30);
    let pager = pager.expect("a pager below the options");
    let pages = 30usize.div_ceil(per_page);
    for _ in 1..pages {
        run(&h, |cx| screen.on_gesture(tap(470, middle(pager).y), cx));
    }
    let (rows, _) = rows_of(&h, &mut screen);
    let last = *rows.last().unwrap();
    let (_, commands) = run(&h, |cx| screen.on_gesture(tap(100, middle(last).y), cx));
    assert_eq!(picked(&commands).as_deref(), Some("o29"));
    // A swipe turns a page while the focus is hidden.
    let mut screen = Choice::new(&h.env(), &opened(&options));
    run(&h, |cx| screen.on_action(&actions::NEXT_PAGE, cx));
    let (rows, _) = rows_of(&h, &mut screen);
    let (_, commands) = run(&h, |cx| screen.on_gesture(tap(100, middle(rows[0]).y), cx));
    let first = format!("o{per_page}");
    assert_eq!(picked(&commands).as_deref(), Some(first.as_str()));
    // By buttons: the page buttons step a row, down to the last.
    let h = Harness::new(Device::BUTTONS);
    let mut screen = Choice::new(&h.env(), &opened(&options));
    for _ in 0..29 {
        run(&h, |cx| screen.on_action(&actions::NEXT_PAGE, cx));
    }
    let (_, commands) = run(&h, |cx| screen.on_action(&actions::CONFIRM, cx));
    assert_eq!(picked(&commands).as_deref(), Some("o29"));
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
    let h = Harness::new(Device::TOUCH);
    let mut screen = Choice::new(&h.env(), &opened(&["open", "delete"]));
    let (_, commands) = run(&h, |cx| screen.on_action(&actions::CONFIRM, cx));
    assert_eq!(picked(&commands), None);
    let (_, commands) = run(&h, |cx| screen.on_action(&actions::CONFIRM, cx));
    assert_eq!(picked(&commands).as_deref(), Some("open"));

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

/// A small tablet on its side and the X4 on its side: width, height, dots
/// per inch (as in the SDK's `ui_responsive` tests).
const X4_SIDE: (u16, u16, u16) = (800, 480, 220);
const TABLET_SIDE: (u16, u16, u16) = (1920, 1200, 220);

/// A device `width` by `height` pixels at `dpi`.
fn harness((width, height, dpi): (u16, u16, u16)) -> Harness {
    use paperpaw_platform::Rotation;
    use paperpaw_sdk::{DisplayInfo, InputCaps, MemoryClass, PixelFormat};
    let mut h = Harness::new(Device::TOUCH);
    h.device = paperpaw_sdk::DeviceInfo::new(
        "test",
        "Test device",
        DisplayInfo::new(width, height, dpi, PixelFormat::Gray8, Rotation::Deg0, None),
        InputCaps::new(true, true, false, &[]),
        MemoryClass::Large,
    );
    h
}

fn whole((width, height, _): (u16, u16, u16)) -> Rect {
    Rect::new(0, 0, width, height)
}

#[test]
fn a_choice_on_a_wide_screen_is_a_sheet_in_the_column_and_a_tap_beside_it_closes_it() {
    use paperpaw_sdk::Outside;
    use paperpaw_sdk::event::Event;
    let h = harness(TABLET_SIDE);
    let env = h.env();
    let style = UiStyle::new(&env);
    let Command::OpenSlot(_, params) =
        paperpaw_sdk::ask::choice("Sort by", &Event::<Picked>::new("t.chosen"))
            .options("t.option.", &["a", "b", "c"])
            .command()
    else {
        panic!("not a slot");
    };
    let choice = Choice::new(&env, &params);
    let region = choice.region(&env).expect("a sheet");
    let column = paperpaw_sdk::ui::readable(&env, &style, whole(TABLET_SIDE));
    // The sheet stands in the column, along the bottom.
    assert_eq!((region.x, region.width), (column.x, column.width));
    assert_eq!(region.bottom(), u32::from(whole(TABLET_SIDE).height));
    // Beside it is outside the sheet, as above it: the overlay's default,
    // a tap there closes it (the screen below is dimmed, not usable).
    for x in [5, i32::from(region.x) - 1, region.right() as i32 + 1, 1915] {
        assert!(!region.contains(paperpaw_sdk::Point::new(x, 1100)), "{x}");
    }
    assert_eq!(choice.outside(), Outside::Close);
    // On the X4 the sheet is as wide as the screen: nothing beside it.
    let x4 = harness(X4_SIDE);
    let env = x4.env();
    let region = Choice::new(&env, &params).region(&env).expect("a sheet");
    assert_eq!((region.x, region.width), (0, 800));
}
