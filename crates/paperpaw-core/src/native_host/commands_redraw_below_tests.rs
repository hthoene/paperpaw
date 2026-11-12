//! `redraw` and `redraw_area` of a screen that has only overlays above it
//! (`screens-input.md` 3): accepted, drawn in that screen's own region, the
//! overlays over it drawn again; a full screen or a guard above refuses it.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
use super::*;
use alloc::boxed::Box;
use alloc::rc::Rc;
use core::cell::Cell;
use paperpaw_native::host_glue::ERR_STATE;
use paperpaw_platform::{Color, Instant, Rect, RefreshMode};
use paperpaw_sdk::{
    Canvas, Cx, Env, Handled, Manifest, Params, Plugin, RegisterError, Registrar, Screen,
    ScreenKey, ScreenSpec, Version,
};

/// A screen of one of the kinds the tests stack: a full page that is black or
/// white, a full black screen, a black bar (an overlay in the lower half), a
/// pin dialog (an overlay that guards).
struct Probe {
    kind: &'static str,
    white: Rc<Cell<bool>>,
}

impl Screen for Probe {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Env<'_>) {
        let color = match self.kind {
            "page" if self.white.get() => Color::WHITE,
            _ => Color::BLACK,
        };
        if matches!(self.kind, "bar" | "dimbar" | "pin") {
            canvas.fill_rect(0, 45, 90, 45, color);
        } else {
            canvas.fill_rect(0, 0, 90, 90, color);
        }
    }

    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        matches!(self.kind, "bar" | "dimbar" | "pin").then(|| Rect::new(0, 45, 90, 45))
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn guards(&self) -> bool {
        self.kind == "pin"
    }

    fn dims_below(&self) -> bool {
        self.kind == "dimbar"
    }
}

struct Probes(Rc<Cell<bool>>);

impl Plugin for Probes {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("probe"), "Probe", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        for (id, kind) in [
            ("probe.page", "page"),
            ("probe.full", "full"),
            ("probe.bar", "bar"),
            ("probe.dimbar", "dimbar"),
            ("probe.pin", "pin"),
        ] {
            let white = Rc::clone(&self.0);
            r.add_screen(ScreenSpec::new(
                Id::new_static(id),
                "probe.title",
                Box::new(move |_, _| {
                    Box::new(Probe {
                        kind,
                        white: Rc::clone(&white),
                    })
                }),
            ))?;
        }
        Ok(())
    }
}

/// A kernel with the screens `ids` opened, bottom first, drawn once.
fn stacked(ids: &[&'static str]) -> (crate::Kernel, Rc<Cell<bool>>) {
    let white = Rc::new(Cell::new(false));
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(Probes(Rc::clone(&white)))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    for (n, id) in ids.iter().enumerate() {
        kernel.open_screen(Instant(n as u64 + 1), Id::new_static(id), Params::new());
        kernel.render(&mut |_, _| {});
    }
    (kernel, white)
}

/// The key of the page (the kernel's own home screen lies below it).
fn page_key(kernel: &crate::Kernel) -> ScreenKey {
    kernel
        .shared
        .stack
        .iter()
        .find(|entry| entry.screen.as_str() == "probe.page")
        .unwrap()
        .key
}

fn redraw(kernel: &crate::Kernel, command: &WireCommand<'_>) -> i32 {
    let env = kernel.shared.env(Instant(0));
    let owner = Id::new_static("probe");
    let mut stage = CommandStage::new(&owner, env.settings(), None, 16, 4096).with_ui(
        &env,
        Some(page_key(kernel)),
        0,
        100,
    );
    let mut bytes = [0; 256];
    let len = commands_encode(core::slice::from_ref(command), &mut bytes).unwrap();
    stage.stage(&commands_decode(&bytes[..len]).unwrap())
}

const WHOLE: WireCommand<'static> = WireCommand::Redraw { mode: 2 };
const AREA: WireCommand<'static> = WireCommand::RedrawArea {
    mode: 2,
    x: 3,
    y: 2,
    width: 10,
    height: 10,
};

#[test]
fn a_page_under_an_overlay_may_redraw_but_not_under_a_full_screen_or_a_guard() {
    for command in [WHOLE, AREA] {
        let (kernel, _) = stacked(&["probe.page"]);
        assert_eq!(redraw(&kernel, &command), OK, "on top");
        let (kernel, _) = stacked(&["probe.page", "probe.bar"]);
        assert_eq!(redraw(&kernel, &command), OK, "under a bar");
        let (kernel, _) = stacked(&["probe.page", "probe.full"]);
        assert_eq!(redraw(&kernel, &command), ERR_STATE, "under a full screen");
        let (kernel, _) = stacked(&["probe.page", "probe.full", "probe.bar"]);
        assert_eq!(
            redraw(&kernel, &command),
            ERR_STATE,
            "a full screen between"
        );
        // A guard above it counts, an overlay or not (`ui.md` 4.3).
        let (kernel, _) = stacked(&["probe.page", "probe.pin"]);
        assert_eq!(redraw(&kernel, &command), ERR_STATE, "under a guard");
    }
}

#[test]
fn a_page_redrawn_under_an_overlay_shows_and_the_overlay_stays_over_it() {
    let (mut kernel, white) = stacked(&["probe.page", "probe.bar"]);
    let owner = Id::new_static("probe");
    assert_eq!(kernel.frame().pixel(5, 10), Color::BLACK);
    assert_eq!(kernel.frame().pixel(5, 60), Color::BLACK, "the bar");
    // The page changes (a picture came) and asks to be drawn again.
    white.set(true);
    kernel.drain_native_ui(
        &owner,
        Vec::new(),
        Some(Redraw {
            screen: page_key(&kernel),
            mode: Some(RefreshMode::Partial),
            whole: true,
            areas: Vec::new(),
        }),
    );
    assert!(kernel.frame.pending.is_some(), "the page is to be drawn");
    kernel.frame.unrendered = kernel.frame.pending.take();
    kernel.render(&mut |_, _| {});
    assert_eq!(kernel.frame().pixel(5, 10), Color::WHITE, "the page");
    assert_eq!(kernel.frame().pixel(5, 60), Color::BLACK, "the bar again");
}

#[test]
fn an_area_under_an_overlay_lands_at_the_pages_offset_not_the_overlays() {
    let (mut kernel, _) = stacked(&["probe.page", "probe.bar"]);
    let owner = Id::new_static("probe");
    let content = kernel.content();
    kernel.frame.pending = None;
    kernel.frame.unrendered = None;
    kernel.drain_native_ui(
        &owner,
        Vec::new(),
        Some(Redraw {
            screen: page_key(&kernel),
            mode: Some(RefreshMode::Partial),
            whole: false,
            areas: alloc::vec![super::Area::new(3, 2, 10, 10).unwrap()],
        }),
    );
    // From the page's corner, not from the bar's (45 rows lower).
    assert_eq!(
        kernel.frame.pending,
        Some(Rect::new(content.x + 3, content.y + 2, 10, 10))
    );
}

#[test]
fn a_deferred_redraw_of_a_covered_page_is_dropped() {
    for ids in [["probe.page", "probe.full"], ["probe.page", "probe.pin"]] {
        let (mut kernel, white) = stacked(&ids);
        let owner = Id::new_static("probe");
        white.set(true);
        kernel.frame.pending = None;
        kernel.frame.unrendered = None;
        kernel.output.redraw = false;
        kernel.drain_native_ui(
            &owner,
            Vec::new(),
            Some(Redraw {
                screen: page_key(&kernel),
                mode: Some(RefreshMode::Partial),
                whole: true,
                areas: Vec::new(),
            }),
        );
        assert!(!kernel.output.redraw);
        assert_eq!(kernel.frame.pending, None, "{ids:?}");
    }
}

#[test]
fn a_page_redrawn_under_a_dimming_overlay_stays_dimmed() {
    let (mut kernel, _) = stacked(&["probe.page", "probe.dimbar"]);
    let owner = Id::new_static("probe");
    // The black page is washed out below the dimming bar.
    let dimmed = kernel.frame().pixel(5, 10);
    assert_ne!(dimmed, Color::BLACK, "dimmed");
    assert_eq!(kernel.frame().pixel(5, 60), Color::BLACK, "the bar");
    for areas in [
        Vec::new(),
        alloc::vec![super::Area::new(0, 0, 90, 90).unwrap()],
    ] {
        let whole = areas.is_empty();
        kernel.drain_native_ui(
            &owner,
            Vec::new(),
            Some(Redraw {
                screen: page_key(&kernel),
                mode: Some(RefreshMode::Partial),
                whole,
                areas,
            }),
        );
        assert!(kernel.frame.pending.is_some(), "drawn again");
        kernel.frame.unrendered = kernel.frame.pending.take();
        kernel.render(&mut |_, _| {});
        assert_eq!(kernel.frame().pixel(5, 10), dimmed, "still dimmed");
        assert_eq!(kernel.frame().pixel(5, 60), Color::BLACK, "the bar again");
    }
}

#[test]
fn the_areas_asked_with_a_whole_redraw_follow_in_the_pages_own_region() {
    let (mut kernel, _) = stacked(&["probe.page", "probe.bar"]);
    let owner = Id::new_static("probe");
    let content = kernel.content();
    let area = super::Area::new(3, 2, 10, 10).unwrap();
    kernel.drain_native_ui(
        &owner,
        Vec::new(),
        Some(Redraw {
            screen: page_key(&kernel),
            mode: Some(RefreshMode::Partial),
            whole: true,
            areas: alloc::vec![area],
        }),
    );
    assert!(kernel.frame.followup.is_some(), "kept for the next frame");
    // The page was drawn: the follow-up draws its area from the page's corner.
    kernel.output.redraw = false;
    kernel.frame.pending = None;
    kernel.frame.unrendered = None;
    kernel.run_followup();
    assert_eq!(
        kernel.frame.pending,
        Some(Rect::new(content.x + 3, content.y + 2, 10, 10))
    );
    // A follow-up of a page a full screen has covered since is dropped.
    let (mut kernel, _) = stacked(&["probe.page", "probe.full"]);
    kernel.frame.followup = Some(super::Followup {
        screen: page_key(&kernel),
        mode: Some(RefreshMode::Partial),
        areas: alloc::vec![area],
    });
    kernel.output.redraw = false;
    kernel.frame.pending = None;
    kernel.frame.unrendered = None;
    kernel.run_followup();
    assert_eq!(kernel.frame.pending, None);
    assert!(kernel.frame.followup.is_none());
}

#[test]
fn areas_of_a_screen_that_is_gone_are_placed_against_the_top() {
    let (mut kernel, _) = stacked(&["probe.page", "probe.bar"]);
    let content = kernel.content();
    kernel.frame.pending = None;
    // Not on the stack: the region of the top (the bar, 45 rows down), as before.
    let area = super::Area::new(3, 2, 10, 10).unwrap();
    kernel.redraw_areas(ScreenKey::new(9999), &[area], None);
    assert_eq!(
        kernel.frame.pending,
        Some(Rect::new(content.x + 3, content.y + 45 + 2, 10, 10))
    );
}
