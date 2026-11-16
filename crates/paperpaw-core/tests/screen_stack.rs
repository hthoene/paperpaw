//! The screen stack, overlays and drawing: screens and overlays opened
//! and closed in any order never empty the stack, never ask for a refresh
//! outside the screen and never pass rows outside it, whole or in bands
//! (ADR 0012); overlays of any size, the screen resized to odd sizes and
//! full refreshes every so many partial ones; frames drawn in bands the
//! same as drawn whole.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]
// Tests panic on purpose when something is wrong.

mod common;

use common::AddDriver;

use common::{BASE, DEVICE, DIMS, FULL, Harness, OVERLAY, REGION, SCREEN, pointer, tap, within};
use paperpaw_core::sdk::builtin::slots;
use paperpaw_core::sdk::{Command, Params};
use paperpaw_core::{Effects, Kernel};
use paperpaw_graphics::Framebuffer;
use paperpaw_platform::{Instant, PointerPhase, PointerTool, Rect, RefreshMode};
use proptest::prelude::*;
use proptest::test_runner::Config;

fn config(cases: u32) -> Config {
    Config {
        failure_persistence: None,
        ..Config::with_cases(
            std::env::var("PROPTEST_CASES")
                .ok()
                .and_then(|n| n.parse().ok())
                .unwrap_or(cases),
        )
    }
}

/// A kernel with the driver, drawn whole or in bands of `rows` aligned
/// to `align`.
fn kernel(harness: &Harness, bands: Option<(u16, u16)>) -> Kernel {
    let builder = Kernel::builder(DEVICE).driver(harness.plugin());
    let builder = match bands {
        Some((rows, align)) => builder.frame_rows(rows).row_align(align),
        None => builder,
    };
    let mut kernel = builder.build().unwrap();
    kernel.start(Instant(0));
    draw(&mut kernel, bands.map_or(1, |(_, align)| align));
    harness.take();
    kernel
}

/// Renders, checking that rows passed lie on the screen, start and end
/// at multiples of `align` (but at the bottom) and do not overlap.
fn draw(kernel: &mut Kernel, align: u16) {
    let bands = common::render(kernel);
    let height = kernel.device().display.height;
    let mut below = 0;
    for (top, rows) in bands {
        let end = top + rows;
        assert!(top >= below, "band at {top} overlaps the one before");
        assert_eq!(top % align, 0, "band at {top} not aligned to {align}");
        assert!(
            end % align == 0 || end == height,
            "band ends at {end}, not aligned to {align}"
        );
        below = end;
    }
}

fn overlay(region: Rect, dims: bool) -> Command {
    let mut params = Params::new().with(
        REGION,
        format!(
            "{},{},{},{}",
            region.x, region.y, region.width, region.height
        ),
    );
    if dims {
        params = params.with(DIMS, "1");
    }
    Command::OpenScreen(OVERLAY, params)
}

#[derive(Debug, Clone)]
enum Op {
    Overlay(Rect, bool),
    Full,
    Back,
    Home,
    Menu,
    Tap(i32, i32),
    Redraw(RefreshMode),
    Notify,
    /// Back and an overlay opened in one go, as a menu that opens another.
    BackAndOverlay(Rect),
}

/// Regions of overlays: inside the screen, partly or wholly off it,
/// empty, as large as it or larger.
fn region() -> impl Strategy<Value = Rect> {
    prop_oneof![
        (0u16..120, 0u16..160, 1u16..120, 1u16..160).prop_map(|(x, y, w, h)| {
            let w = w.min(120 - x).max(1);
            let h = h.min(160 - y).max(1);
            Rect::new(x, y, w, h)
        }),
        Just(Rect::new(0, 80, 120, 80)),
        Just(Rect::new(0, 0, 120, 160)),
        Just(Rect::new(119, 159, 1, 1)),
        Just(Rect::new(0, 0, 0, 0)),
    ]
}

fn op() -> impl Strategy<Value = Op> {
    let mode = prop_oneof![
        Just(RefreshMode::Full),
        Just(RefreshMode::Partial),
        Just(RefreshMode::Fast),
    ];
    prop_oneof![
        4 => (region(), any::<bool>()).prop_map(|(r, d)| Op::Overlay(r, d)),
        2 => Just(Op::Full),
        4 => Just(Op::Back),
        1 => Just(Op::Home),
        2 => Just(Op::Menu),
        3 => (-10i32..130, -10i32..170).prop_map(|(x, y)| Op::Tap(x, y)),
        1 => mode.prop_map(Op::Redraw),
        1 => Just(Op::Notify),
        1 => region().prop_map(Op::BackAndOverlay),
    ]
}

/// Carries out `op` at `now`; returns its effects.
fn apply(harness: &Harness, kernel: &mut Kernel, now: u64, op: &Op) -> Effects {
    let open_slot = |slot| Command::OpenSlot(slot, Params::new());
    let commands = match op {
        Op::Overlay(region, dims) => vec![overlay(*region, *dims)],
        Op::Full => vec![Command::OpenScreen(FULL, Params::new())],
        Op::Back => vec![Command::Back],
        Op::Home => vec![open_slot(slots::HOME)],
        Op::Menu => vec![open_slot(slots::MENU)],
        Op::Tap(x, y) => {
            let touch = |phase| pointer(0, phase, *x, *y, PointerTool::Finger);
            kernel.handle(Instant(now), touch(PointerPhase::Down));
            return kernel.handle(Instant(now + 60), touch(PointerPhase::Up));
        }
        Op::Redraw(mode) => vec![Command::Redraw(*mode)],
        Op::Notify => vec![Command::Notify(String::from("Saved"))],
        Op::BackAndOverlay(region) => vec![Command::Back, overlay(*region, true)],
    };
    harness.run(kernel, now, commands)
}

fn check(kernel: &Kernel, effects: Effects, ops: &[Op]) {
    assert!(kernel.active_screen().is_some(), "no screen after {ops:?}");
    assert_eq!(
        kernel.screens().next(),
        Some(&BASE),
        "the home screen left the bottom after {ops:?}"
    );
    if let Some((region, _)) = effects.refresh() {
        assert!(
            within(region, SCREEN),
            "refresh {region:?} off the screen after {ops:?}"
        );
    }
}

proptest! {
    #![proptest_config(config(160))]

    /// Overlays and screens opened and closed in any order, drawn whole or
    /// in bands.
    #[test]
    fn screens_open_and_close_in_any_order(
        ops in prop::collection::vec(op(), 0..30),
        bands in prop_oneof![
            Just(None),
            Just(Some((16, 8))),
            Just(Some((7, 1))),
            Just(Some((1, 1))),
            Just(Some((160, 16))),
        ],
    ) {
        let harness = Harness::new();
        let mut kernel = kernel(&harness, bands);
        let align = bands.map_or(1, |(_, align)| align);
        for (i, op) in ops.iter().enumerate() {
            let now = 1000 + i as u64 * 1000;
            let effects = apply(&harness, &mut kernel, now, op);
            check(&kernel, effects, &ops[..=i]);
            draw(&mut kernel, align);
            // Notices go by themselves.
            let effects = kernel.tick(Instant(now + 900));
            check(&kernel, effects, &ops[..=i]);
            draw(&mut kernel, align);
        }
        // Back as often as there are screens: home is left.
        for i in 0..40 {
            harness.run(&mut kernel, 100_000 + i, [Command::Back]);
        }
        draw(&mut kernel, align);
        prop_assert_eq!(kernel.screens().count(), 1);
    }

    /// Frames drawn in bands are the frames drawn whole (ADR 0012), as a
    /// panel controller that keeps the frame shows them.
    #[test]
    fn banded_frames_match_whole_frames(
        ops in prop::collection::vec(op(), 0..20),
        bands in prop_oneof![Just((16, 8)), Just((7, 1)), Just((1, 1)), Just((40, 8))],
    ) {
        let (whole_harness, banded_harness) = (Harness::new(), Harness::new());
        let mut whole = kernel(&whole_harness, None);
        let mut banded = kernel(&banded_harness, Some(bands));
        let panel = || Framebuffer::new(120, 160, DEVICE.display.format).unwrap();
        let (mut whole_panel, mut banded_panel) = (panel(), panel());
        let show = |kernel: &mut Kernel, panel: &mut Framebuffer| {
            kernel.render(&mut |top, rows| panel.write_rows(top, rows));
        };
        // Both start from the same first frame.
        whole.draw_again(Instant(500));
        banded.draw_again(Instant(500));
        show(&mut whole, &mut whole_panel);
        show(&mut banded, &mut banded_panel);
        for (i, op) in ops.iter().enumerate() {
            let now = 1000 + i as u64 * 1000;
            apply(&whole_harness, &mut whole, now, op);
            apply(&banded_harness, &mut banded, now, op);
            show(&mut whole, &mut whole_panel);
            show(&mut banded, &mut banded_panel);
            prop_assert!(
                whole_panel.view().data == banded_panel.view().data,
                "frames differ after {:?}",
                &ops[..=i]
            );
        }
    }

    /// Every screen opened is closed once at most, and only screens
    /// opened are.
    #[test]
    fn screens_close_once(ops in prop::collection::vec(op(), 0..30)) {
        let harness = Harness::new();
        let mut kernel = kernel(&harness, None);
        for (i, op) in ops.iter().enumerate() {
            apply(&harness, &mut kernel, 1000 + i as u64 * 1000, op);
            common::render(&mut kernel);
        }
        let log = harness.take();
        let closed = log.iter().filter(|l| l.ends_with(":close")).count();
        let opened = ops
            .iter()
            .map(|op| match op {
                Op::Overlay(..) | Op::Full | Op::Menu | Op::BackAndOverlay(_) => 1,
                Op::Tap(..) => 1,
                _ => 0,
            })
            .sum::<usize>();
        prop_assert!(closed <= opened, "{closed} closed, at most {opened} opened: {log:?}");
        prop_assert!(!log.iter().any(|l| l == "base:close"), "{log:?}");
    }
}

/// An overlay that says it reaches past the screen is refreshed only on
/// the screen.
#[test]
fn an_overlay_reaching_past_the_screen_refreshes_only_the_screen() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, None);
    for region in [
        Rect::new(100, 140, 100, 100),
        Rect::new(0, 0, u16::MAX, u16::MAX),
        Rect::new(500, 500, 10, 10),
    ] {
        let effects = harness.run(&mut kernel, 1000, [overlay(region, false)]);
        let refresh = effects.refresh().map(|(r, _)| r);
        assert!(
            refresh.is_none_or(|r| within(r, SCREEN)),
            "an overlay at {region:?} refreshes {refresh:?}"
        );
        common::render(&mut kernel);
        let effects = harness.run(&mut kernel, 2000, [Command::Back]);
        let refresh = effects.refresh().map(|(r, _)| r);
        assert!(
            refresh.is_none_or(|r| within(r, SCREEN)),
            "closing an overlay at {region:?} refreshes {refresh:?}"
        );
        common::render(&mut kernel);
    }
}

/// The rows passed for an overlay reaching past the screen stay on it.
#[test]
fn an_overlay_reaching_past_the_screen_draws_only_on_the_screen() {
    for bands in [None, Some((16, 8))] {
        let harness = Harness::new();
        let mut kernel = kernel(&harness, bands);
        for region in [
            Rect::new(100, 140, 100, 100),
            Rect::new(0, 0, u16::MAX, u16::MAX),
            Rect::new(500, 500, 10, 10),
        ] {
            harness.run(&mut kernel, 1000, [overlay(region, true)]);
            draw(&mut kernel, bands.map_or(1, |(_, a)| a));
            harness.run(&mut kernel, 2000, [Command::Back]);
            draw(&mut kernel, bands.map_or(1, |(_, a)| a));
        }
        assert_eq!(kernel.active_screen(), Some(&BASE));
    }
}

#[test]
fn a_tap_outside_an_overlay_closes_only_it() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, None);
    harness.run(
        &mut kernel,
        1000,
        [
            Command::OpenScreen(FULL, Params::new()),
            overlay(Rect::new(0, 100, 120, 60), false),
            overlay(Rect::new(0, 130, 120, 30), false),
        ],
    );
    assert_eq!(kernel.screens().count(), 4);
    tap(&mut kernel, 2000, 60, 110);
    assert_eq!(kernel.screens().count(), 3);
    assert_eq!(kernel.active_screen(), Some(&OVERLAY));
    tap(&mut kernel, 3000, 60, 20);
    assert_eq!(kernel.active_screen(), Some(&FULL));
}

#[test]
fn back_on_the_last_screen_keeps_it() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, None);
    for i in 0..5 {
        let effects = harness.run(&mut kernel, 1000 + i, [Command::Back, Command::Back]);
        assert!(effects.refresh().is_none());
    }
    assert_eq!(kernel.active_screen(), Some(&BASE));
    assert!(harness.lines("base:close").is_empty());
}

#[test]
fn opening_screens_that_do_not_exist_leaves_the_stack() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, None);
    harness.run(
        &mut kernel,
        1000,
        [
            Command::OpenScreen(
                paperpaw_core::sdk::Id::new_static("nobody.screen"),
                Params::new(),
            ),
            Command::OpenSlot(
                paperpaw_core::sdk::Id::new_static("nobody.slot"),
                Params::new(),
            ),
        ],
    );
    assert_eq!(kernel.screens().count(), 1);
    let warnings = kernel.take_warnings();
    assert_eq!(warnings.len(), 2, "{warnings:?}");
}

#[test]
fn an_action_that_runs_itself_for_ever_is_cut_off() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, None);
    for i in 0..3 {
        harness.run(&mut kernel, 1000 + i, [Command::run(common::LOOP)]);
    }
    let warnings = kernel.take_warnings();
    assert_eq!(warnings.len(), 3, "{warnings:?}");
    // Cut off at once, as it runs itself: nothing else is dropped.
    assert!(warnings[0].message.contains("runs itself"), "{warnings:?}");
    // The kernel goes on.
    tap(&mut kernel, 2000, 60, 80);
    assert_eq!(kernel.active_screen(), Some(&OVERLAY));
}

#[test]
fn every_so_many_partial_refreshes_one_is_full() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, None);
    kernel
        .set_setting(Instant(10), "core.full-refresh-interval", "3")
        .unwrap();
    common::render(&mut kernel);
    let mut modes = Vec::new();
    for i in 0..9 {
        let effects = harness.run(
            &mut kernel,
            1000 + i,
            [Command::Redraw(RefreshMode::Partial)],
        );
        modes.push(effects.refresh().unwrap());
        common::render(&mut kernel);
    }
    let full: Vec<usize> = modes
        .iter()
        .enumerate()
        .filter(|(_, (_, mode))| *mode == RefreshMode::Full)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(full, [2, 5, 8], "{modes:?}");
    // A full one covers the whole screen.
    assert!(
        modes
            .iter()
            .filter(|(_, mode)| *mode == RefreshMode::Full)
            .all(|(region, _)| *region == SCREEN)
    );
    // Fast ones do not count.
    for i in 0..5 {
        let effects = harness.run(&mut kernel, 2000 + i, [Command::Redraw(RefreshMode::Fast)]);
        assert_eq!(effects.refresh().map(|(_, m)| m), Some(RefreshMode::Fast));
        common::render(&mut kernel);
    }
}

#[test]
fn resizing_to_odd_sizes_keeps_drawing_on_the_screen() {
    for bands in [None, Some((16, 8)), Some((5, 1))] {
        let harness = Harness::new();
        let mut kernel = kernel(&harness, bands);
        harness.run(&mut kernel, 500, [overlay(Rect::new(0, 80, 120, 80), true)]);
        for (i, (width, height)) in [
            (1, 1),
            (1, 400),
            (400, 1),
            (0, 100),
            (100, 0),
            (7, 13),
            (160, 120),
            (120, 160),
        ]
        .into_iter()
        .enumerate()
        {
            let effects = kernel.resize(Instant(1000 + i as u64), width, height);
            let display = kernel.device().display;
            if width > 0 && height > 0 {
                assert_eq!((display.width, display.height), (width, height));
            }
            let screen = Rect::new(0, 0, display.width, display.height);
            if let Some((region, _)) = effects.refresh() {
                assert!(within(region, screen), "{region:?} on {screen:?}");
            }
            draw(&mut kernel, 1);
            assert_eq!(kernel.screens().count(), 2);
        }
    }
}

#[test]
fn the_screen_is_described_and_captured_whatever_is_open() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, None);
    harness.run(
        &mut kernel,
        1000,
        [
            overlay(Rect::new(0, 0, 0, 0), true),
            overlay(Rect::new(100, 150, 300, 300), false),
        ],
    );
    // Drawn aside for reading aloud.
    let _ = kernel.describe();
    // Not captured while changes wait to be rendered; then whole.
    assert!(!kernel.capture(&mut |_, _| {}));
    common::render(&mut kernel);
    let mut rows = 0;
    assert!(kernel.capture(&mut |_, view| rows += view.height));
    assert_eq!(rows, 160);
}
