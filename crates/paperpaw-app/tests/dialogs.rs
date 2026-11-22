//! The choice and the yes or no question the kernel asks (`ask::choice`,
//! `ask::confirm`), answered on the dialog plugin's screens (the keyboard's
//! `keyboard.ask`, #293), by buttons and by touch
//! alike: the answer comes back as the reply event the question named, with
//! its context; *Back* or a tap outside answers nothing. (Moved here from the
//! kernel's stock screens, `paperpaw-core`'s `tests/stock_screens.rs` and
//! `tests/kernel_api.rs`.)

#![cfg(all(
    feature = "native-linked",
    feature = "native-linked-reading",
    feature = "native-linked-store"
))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod native_activation;

#[path = "../../paperpaw-core/tests/common/mod.rs"]
mod common;

use common::{AddDriver as _, Harness};
use paperpaw_core::Ask;
use paperpaw_core::Kernel;
use paperpaw_core::kit::ui::choice::Picked;
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rect, Rotation,
};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::Confirmed;
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::{Id, MemoryStorage};

/// The emulator's `xteink-x4`: buttons only.
const X4: DeviceInfo = DeviceInfo::new(
    "xteink-x4",
    "Xteink X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg90, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);

/// The emulator's `boox-go-7`: touch and buttons.
const BOOX: DeviceInfo = DeviceInfo::new(
    "boox-go-7",
    "Boox Go 7",
    DisplayInfo::new(1264, 1680, 300, PixelFormat::Gray8, Rotation::Deg0, None),
    InputCaps::new(true, true, false, &[]),
    MemoryClass::Large,
);

const ANSWERED: Event<Confirmed> = Event::new("drv.reply");
const CHOSEN: Event<Picked> = Event::new("drv.reply");

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

/// The standard app on `device` with the test driver, past its first-run setup.
fn app(harness: &Harness, device: DeviceInfo) -> Kernel {
    let storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
    let (builder, card) = native_activation::quiet_on(device, storage, "[core]\nlanguage = en\n");
    let builder = paperpaw_app::install(builder, fonts()).driver(harness.plugin());
    let mut kernel = native_activation::build_on(builder, card);
    let _ = kernel.start(Instant(0));
    common::render(&mut kernel);
    harness.take();
    kernel
}

/// Asks what `command` asks (an `ask::choice` or `ask::confirm`) as the
/// kernel's code does, and draws the dialog.
fn ask(kernel: &mut Kernel, at: u64, command: Ask) {
    let Ask::OpenSlot(slot, params) = command else {
        panic!("a slot");
    };
    let _ = kernel.open_slot(Instant(at), slot, params);
    common::render(kernel);
}

fn screen(kernel: &Kernel) -> &str {
    kernel.active_screen().map_or("", Id::as_str)
}

fn run(kernel: &mut Kernel, at: u64, action: Id) {
    let _ = kernel.run_action(Instant(at), action);
    common::render(kernel);
}

/// The parts of `role` the dialog on top shows (what lies below it is
/// described too): label, place, focused.
fn parts(kernel: &mut Kernel, role: Role) -> Vec<(String, Rect, bool)> {
    common::render(kernel);
    let parts = kernel.describe();
    let from = parts
        .iter()
        .rposition(|p| p.role == Role::Dialog)
        .unwrap_or(0);
    parts[from..]
        .iter()
        .filter(|p| p.role == role)
        .map(|p| (p.label.clone(), p.area, p.focused))
        .collect()
}

/// Lets timers run until nothing is due within a second of `now`.
fn settle(kernel: &mut Kernel, mut effects: paperpaw_core::Effects, mut now: u64) {
    for _ in 0..1000 {
        let Some(at) = effects.wake_at() else {
            return;
        };
        if at.0 > now + 1000 {
            return;
        }
        now = now.max(at.0);
        effects = kernel.tick(Instant(now));
    }
}

/// A tap with a finger at `(x, y)` at `at`, and what it leads to.
fn tap(kernel: &mut Kernel, at: u64, x: i32, y: i32) {
    use paperpaw_platform::{InputEvent, Point, PointerEvent, PointerPhase, PointerTool};
    for (phase, when) in [(PointerPhase::Down, at), (PointerPhase::Up, at + 60)] {
        let event = InputEvent::Pointer(PointerEvent::new(
            0,
            phase,
            Point::new(x, y),
            PointerTool::Finger,
            1000,
        ));
        let effects = kernel.handle(Instant(when), event);
        // Timers wait while the finger is down: a long press is not meant.
        if phase == PointerPhase::Up {
            settle(kernel, effects, when);
        }
    }
    common::render(kernel);
}

fn middle(rect: Rect) -> (i32, i32) {
    (
        i32::from(rect.x) + i32::from(rect.width) / 2,
        i32::from(rect.y) + i32::from(rect.height) / 2,
    )
}

/// The replies heard since the last call.
fn replies(harness: &Harness) -> Vec<String> {
    let lines = harness.lines("event drv.reply");
    harness.take();
    lines
}

#[test]
fn a_yes_or_no_question_is_answered_by_buttons_and_by_touch() {
    let harness = Harness::new();
    let mut kernel = app(&harness, BOOX);
    let question = || {
        paperpaw_core::kit::ask::confirm("Delete?", &ANSWERED)
            .with("book", "a.epub")
            .command()
    };
    let answers = |harness: &Harness| -> Vec<bool> {
        replies(harness)
            .iter()
            .map(|l| {
                assert!(l.contains("book=a.epub"), "{l}");
                l.contains("answer=yes")
            })
            .collect()
    };
    // Asked: yes and no, the focus on no.
    ask(&mut kernel, 100, question());
    assert_eq!(screen(&kernel), "keyboard.ask");
    let shown = parts(&mut kernel, Role::Button);
    let labels: Vec<&str> = shown.iter().map(|(l, _, _)| l.as_str()).collect();
    assert_eq!(labels, ["Yes", "No"]);
    assert!(!shown[0].2 && shown[1].2, "{shown:?}");
    // Yes with the buttons: left to it, then confirm.
    run(&mut kernel, 200, actions::LEFT);
    assert!(parts(&mut kernel, Role::Button)[0].2);
    run(&mut kernel, 300, actions::CONFIRM);
    assert_ne!(screen(&kernel), "keyboard.ask");
    assert_eq!(answers(&harness), [true]);
    // No by a tap on it.
    ask(&mut kernel, 400, question());
    let (x, y) = middle(parts(&mut kernel, Role::Button)[1].1);
    tap(&mut kernel, 500, x, y);
    assert_ne!(screen(&kernel), "keyboard.ask");
    assert_eq!(answers(&harness), [false]);
    // Back: closed, no answer.
    ask(&mut kernel, 700, question());
    run(&mut kernel, 800, actions::BACK);
    assert_ne!(screen(&kernel), "keyboard.ask");
    assert_eq!(answers(&harness), Vec::<bool>::new());
    // A tap outside the box, at the top of the screen: closed, no answer.
    ask(&mut kernel, 900, question());
    tap(&mut kernel, 1000, 600, 2);
    assert_ne!(screen(&kernel), "keyboard.ask");
    assert_eq!(answers(&harness), Vec::<bool>::new());
}

/// The option a reply picked, and the path the question was about.
fn picked(harness: &Harness) -> Vec<String> {
    replies(harness)
        .iter()
        .map(|l| {
            assert!(l.contains("path=books/a.epub"), "{l}");
            let option = l
                .split(' ')
                .find_map(|p| p.strip_prefix("option="))
                .expect("an option");
            String::from(option)
        })
        .collect()
}

fn choice(options: &[&str]) -> Ask {
    paperpaw_core::kit::ask::choice("Delete “a.epub”?", &CHOSEN)
        .about("books/a.epub")
        .options("core.confirm.", options)
        .command()
}

#[test]
fn each_option_is_picked_alike_by_tap_and_by_confirm() {
    let options = ["yes", "no", "ok"];
    let harness = Harness::new();
    let mut kernel = app(&harness, BOOX);
    for (index, option) in options.iter().enumerate() {
        // By a tap on its row.
        ask(&mut kernel, 100, choice(&options));
        assert_eq!(screen(&kernel), "keyboard.ask");
        let rows = parts(&mut kernel, Role::Row);
        assert_eq!(rows.len(), options.len(), "{rows:?}");
        let (x, y) = middle(rows[index].1);
        tap(&mut kernel, 200, x, y);
        assert_eq!(picked(&harness), [*option], "tapped");
        // By the buttons: down to it (the first press shows the focus, on
        // the first option), then confirm.
        ask(&mut kernel, 300, choice(&options));
        for at in 0..=index {
            run(&mut kernel, 400 + at as u64, actions::DOWN);
        }
        run(&mut kernel, 500, actions::CONFIRM);
        assert_eq!(picked(&harness), [*option], "confirmed");
    }
}

/// On a touch screen the focus shows only once a button moved it: a first
/// *Confirm* shows it instead of choosing a row nobody saw.
#[test]
fn a_first_confirm_on_a_touch_screen_shows_the_focus() {
    let harness = Harness::new();
    let mut kernel = app(&harness, BOOX);
    ask(&mut kernel, 100, choice(&["yes", "no"]));
    run(&mut kernel, 200, actions::CONFIRM);
    assert_eq!(screen(&kernel), "keyboard.ask");
    assert_eq!(picked(&harness), Vec::<String>::new());
    run(&mut kernel, 300, actions::CONFIRM);
    assert_eq!(picked(&harness), ["yes"]);
}

#[test]
fn back_and_a_tap_outside_pick_nothing() {
    let harness = Harness::new();
    let mut kernel = app(&harness, BOOX);
    ask(&mut kernel, 100, choice(&["yes", "no"]));
    run(&mut kernel, 200, actions::BACK);
    assert_ne!(screen(&kernel), "keyboard.ask");
    ask(&mut kernel, 300, choice(&["yes", "no"]));
    tap(&mut kernel, 400, 600, 2);
    assert_ne!(screen(&kernel), "keyboard.ask");
    assert_eq!(picked(&harness), Vec::<String>::new());
}

/// A choice of no options is not asked: nothing opens, nothing is picked, the
/// log says so.
#[test]
fn a_choice_of_no_options_is_not_asked() {
    let harness = Harness::new();
    let mut kernel = app(&harness, X4);
    let before = screen(&kernel).to_string();
    ask(&mut kernel, 100, choice(&[]));
    assert_eq!(screen(&kernel), before);
    assert_eq!(picked(&harness), Vec::<String>::new());
    assert!(
        kernel
            .take_warnings()
            .iter()
            .any(|w| w.message.contains("could not be asked"))
    );
}

/// More options than fit go on pages, and every one is reached by the buttons
/// alone (the X4) and by a tap on its row on its page.
#[test]
fn every_option_on_pages_is_reached_by_touch_and_by_buttons() {
    let names: Vec<String> = (0..30).map(|n| format!("o{n}")).collect();
    let options: Vec<&str> = names.iter().map(String::as_str).collect();
    // Labelled with the options' own words (none of them is translated).
    let paged = || {
        paperpaw_core::kit::ask::choice("Pick one", &CHOSEN)
            .about("books/a.epub")
            .options("", &options)
            .command()
    };
    let harness = Harness::new();
    let mut kernel = app(&harness, X4);
    ask(&mut kernel, 100, paged());
    assert!(
        parts(&mut kernel, Role::Row).len() < options.len(),
        "the options take more than a page"
    );
    // The last one: the page button to the last page and its end (the arrow
    // keys stay on the page, #465).
    for at in 0..options.len() {
        run(&mut kernel, 200 + at as u64, actions::NEXT_PAGE);
    }
    run(&mut kernel, 300, actions::CONFIRM);
    assert_eq!(picked(&harness), ["o29"]);
    // By touch: the first row of the page the focus moved to.
    let harness = Harness::new();
    let mut kernel = app(&harness, BOOX);
    ask(&mut kernel, 100, paged());
    run(&mut kernel, 150, actions::NEXT_PAGE);
    let rows = parts(&mut kernel, Role::Row);
    let (x, y) = middle(rows[0].1);
    let first = rows[0].0.clone();
    tap(&mut kernel, 200, x, y);
    assert_eq!(picked(&harness), [first]);
}
