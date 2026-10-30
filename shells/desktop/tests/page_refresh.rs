//! A page turned in a book refreshes partially on the X4s, by a button
//! or a tap: no full refresh, and no frame in grays (which a panel of
//! black and white shows with a refresh of the whole screen). Full
//! refreshes come only every few pages, as *Full refresh* says. An X4 Pro
//! flashed at every page in beta.8's tests, with smooth text on.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::sdk::MemoryStorage;
use paperpaw_emulator::{device::Device, profiles};
use paperpaw_platform::{
    ButtonState, InputEvent, Point, PointerEvent, PointerPhase, PointerTool, RefreshMode, buttons,
};

const BOOK: &str = "books/alice-in-wonderland.txt";

/// How a page was shown: its refresh, and whether in grays.
type Shown = Option<(RefreshMode, bool)>;

/// The book open on `profile`, `settings` in its `paperpaw.ini`.
fn reading(profile: &str, settings: &str) -> Device {
    let files = MemoryStorage::new()
        .with(
            BOOK,
            include_str!("../../../samples/library/books/alice-in-wonderland.txt"),
        )
        .with(".paperpaw/plugins/setup/done", "")
        .with(
            ".paperpaw/plugins/reader/state.ini",
            format!("[reader]\nlast = {BOOK}\n"),
        )
        .with("paperpaw.ini", format!("[help]\nshow = false\n{settings}"));
    // Refreshes are checked here, not memory (tests/memory.rs does): the
    // budget counts every thread's allocations at once, and the tests run
    // side by side.
    paperpaw_emulator::budget::override_limit(None);
    let profile = profiles::find(profile).unwrap();
    let mut device = Device::in_memory(profile, None, Rc::new(files), true, None).unwrap();
    device.start();
    device.advance(3_000);
    let mut screen = None;
    device.call(|kernel, _| {
        screen = kernel.active_screen().map(ToString::to_string);
        paperpaw_core::Effects::default()
    });
    assert_eq!(screen.as_deref(), Some("reader.screen"));
    device
}

/// What `events` show, one after the other.
fn shown(device: &mut Device, events: Vec<InputEvent>) -> Shown {
    let mut shown = None;
    for event in events {
        device.call(|kernel, now| {
            let effects = kernel.handle(now, event);
            if let Some((_, mode)) = effects.refresh() {
                shown = Some((mode, effects.grays()));
            }
            effects
        });
        device.advance(400);
    }
    shown
}

/// `turns` pages turned forward on `profile` with `settings`, by the
/// page button or by taps on the right of the page.
fn page_turns(profile: &str, settings: &str, turns: usize, tap: bool) -> Vec<Shown> {
    let mut device = reading(profile, settings);
    (0..turns)
        .map(|_| {
            let events = if tap {
                [PointerPhase::Down, PointerPhase::Up]
                    .into_iter()
                    .map(|phase| {
                        InputEvent::Pointer(PointerEvent::new(
                            0,
                            phase,
                            Point::new(420, 400),
                            PointerTool::Finger,
                            1000,
                        ))
                    })
                    .collect()
            } else {
                [ButtonState::Pressed, ButtonState::Released]
                    .into_iter()
                    .map(|state| InputEvent::Button {
                        name: buttons::PAGE_FORWARD,
                        state,
                    })
                    .collect()
            };
            shown(&mut device, events)
        })
        .collect()
}

/// Every page shown partially, in black and white.
fn partial(profile: &str, tap: bool) {
    let shown = page_turns(profile, "", 4, tap);
    assert!(
        shown
            .iter()
            .all(|s| *s == Some((RefreshMode::Partial, false))),
        "{profile}: pages turned not partially in black and white: {shown:?}"
    );
}

#[test]
fn a_page_turn_refreshes_partially_on_the_x4() {
    partial("xteink-x4", false);
}

#[test]
fn a_page_turn_refreshes_partially_on_the_x4_pro() {
    partial("xteink-x4-pro", false);
    partial("xteink-x4-pro", true);
}

/// Smooth text, where the reader chose it on such a panel: each page in
/// grays, as its help says (slower, more battery).
#[test]
fn smooth_text_shows_pages_in_grays_where_chosen() {
    let shown = page_turns("xteink-x4-pro", "[core]\nsmooth-text = on\n", 2, false);
    assert!(
        shown.iter().all(|s| matches!(s, Some((_, true)))),
        "{shown:?}"
    );
}
