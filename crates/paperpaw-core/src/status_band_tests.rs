//! The status bar (#123): who decides, the part of the display a screen
//! gets, and that a status change draws the band alone.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
use super::shown;
use crate::Staging;
use crate::{Place, Turn};
use alloc::boxed::Box;
use paperpaw_platform::{Battery, Instant, Rect, SystemStatus};
use paperpaw_sdk::{Canvas, Handled, Id, Manifest, Params, RegisterError, Version};

const SHOW: u32 = 1 << 11;
const HIDE: u32 = 2 << 11;

#[test]
fn the_setting_wins_over_the_screen_and_the_screen_over_the_default() {
    // The setting's line for the screen first.
    assert!(shown("a.b=shown", "a.b", 2, false));
    assert!(!shown("x.y=shown\n a.b = hidden ", "a.b", 1, true));
    // Another screen's line, or a value it does not know, leaves it to the
    // screen.
    assert!(shown("x.y=hidden\na.b=maybe", "a.b", 1, false));
    assert!(!shown("", "a.b", 2, true));
    // The screen that says nothing: the device default.
    assert!(!shown("", "a.b", 0, false));
    assert!(shown("", "a.b", 0, true));
    // Past 512 bytes nothing is read.
    let long = ["x=hidden\n".repeat(60), "a.b=shown".into()].concat();
    assert!(!shown(&long, "a.b", 0, false));
}

struct Probe(u32);

impl crate::testing::Fake for Probe {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Place<'_>) {
        canvas.fill_rect(0, 0, 4, 4, paperpaw_platform::Color::BLACK);
    }

    fn flags(&self) -> u32 {
        self.0
    }

    fn on_action(&mut self, _: &Id, _: &mut Turn<'_, '_>) -> Handled {
        Handled::No
    }
}

fn probes(r: &mut Staging) -> Result<(), RegisterError> {
    for (id, flags) in [("probe.old", 0), ("probe.show", SHOW), ("probe.hide", HIDE)] {
        r.add_fake(crate::testing::FakeSpec::new(
            Id::new_static(id),
            "probe.title",
            Box::new(move |_, _| Box::new(Probe(flags))),
        ))?;
    }
    Ok(())
}

fn opened(config: &str, id: &'static str) -> crate::Kernel {
    opened_on(
        crate::Kernel::builder(crate::tests::DEVICE).user_config(config),
        id,
    )
}

fn opened_on(builder: crate::KernelBuilder, id: &'static str) -> crate::Kernel {
    let mut kernel = builder
        .items(
            Manifest::new(Id::new_static("probe"), "Probe", Version::new(1, 0, 0))
                .uses(paperpaw_sdk::Uses::NOTHING),
            probes,
        )
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.open_screen(Instant(1), Id::new_static(id), Params::new());
    kernel.render(&mut |_, _| {});
    kernel
}

/// The top of the part of the display the screen on top has.
fn top(kernel: &crate::Kernel) -> u16 {
    kernel.stack.last().unwrap().area.y
}

#[test]
fn an_old_screen_gets_the_whole_display_unless_the_user_turned_the_bar_on() {
    assert_eq!(top(&opened("", "probe.old")), 0, "the device default");
    assert!(top(&opened("[core]\nstatus-bar.all = on\n", "probe.old")) > 0);
    assert_eq!(
        top(&opened("[core]\nstatus-bar.all = on\n", "probe.hide")),
        0
    );
}

#[test]
fn a_screen_that_shows_the_bar_is_below_it_unless_the_setting_hides_it_there() {
    let kernel = opened("", "probe.show");
    let band = top(&kernel);
    assert!(band > 0);
    // The screen draws from its own corner, below the band.
    assert_eq!(
        kernel.frame().pixel(1, band + 1),
        paperpaw_platform::Color::BLACK
    );
    let config = "[core]\nstatus-bar.screens = probe.show=hidden\n";
    assert_eq!(top(&opened(config, "probe.show")), 0);
    // Changed at run time: the screen moves under the band.
    let mut kernel = opened("[core]\nstatus-bar.all = on\n", "probe.hide");
    kernel
        .set_setting(Instant(2), "core.status-bar.screens", "probe.hide=shown")
        .unwrap();
    assert!(top(&kernel) > 0);
}

#[test]
fn a_status_change_draws_the_band_alone() {
    let mut kernel = opened("", "probe.show");
    let width = kernel.frame().width;
    let band = Rect::new(0, 0, width, top(&kernel));
    kernel.frame.pending = None;
    kernel.frame.unrendered = None;
    kernel.set_status(
        Instant(5),
        SystemStatus::new(None, Some(Battery::new(40, false, None))),
    );
    assert_eq!(kernel.frame.unrendered, Some(band));
    // Without a band nothing is drawn for it.
    let mut kernel = opened("", "probe.old");
    kernel.frame.pending = None;
    kernel.frame.unrendered = None;
    kernel.set_status(
        Instant(5),
        SystemStatus::new(None, Some(Battery::new(40, false, None))),
    );
    assert_eq!(kernel.frame.unrendered, None);
}

#[test]
fn a_run_time_policy_moves_the_screen_and_zero_keeps_what_it_declared() {
    use crate::native_host::commands::PolicyChange;
    let mut kernel = opened("", "probe.old");
    let serial = kernel.stack.last().unwrap().serial;
    let change = |flags| {
        alloc::vec![PolicyChange {
            instance: serial,
            flags,
            ready: false,
        }]
    };
    let probe = Id::new_static("probe");
    kernel.native_screen_policies(&probe, 1, change(SHOW));
    assert!(top(&kernel) > 0, "shown at run time");
    // A policy for another bit (secret) keeps the bar where the screen has it.
    let mut kernel = opened("", "probe.show");
    kernel.native_screen_policies(&probe, 1, change(1 << 4));
    assert!(top(&kernel) > 0);
    kernel.native_screen_policies(&probe, 1, change(HIDE));
    assert_eq!(top(&kernel), 0, "hidden at run time");
}

/// The row of hints set at run time (`hints`, bits 13-14, #448): on a press
/// the screen gets the whole display and the row shows only after one; 0
/// keeps what the screen declared (always, below it).
#[test]
fn a_run_time_hints_policy_gives_the_screen_the_display_and_zero_keeps_the_declared() {
    use crate::native_host::commands::PolicyChange;
    use paperpaw_sdk::HintVisibility;
    let builder = crate::Kernel::builder(crate::tests::DEVICE).button_row(&["back", "confirm"]);
    let mut kernel = opened_on(builder, "probe.old");
    let serial = kernel.stack.last().unwrap().serial;
    let change = |flags| {
        alloc::vec![PolicyChange {
            instance: serial,
            flags,
            ready: false,
        }]
    };
    let height = |kernel: &crate::Kernel| kernel.stack.last().unwrap().area.height;
    let display = kernel.framebuffer.bounds().height;
    let below_the_row = height(&kernel);
    assert!(below_the_row < display, "declared: above the row");
    let probe = Id::new_static("probe");
    kernel.native_screen_policies(&probe, 1, change(2 << 13));
    assert_eq!(kernel.base_hints(), HintVisibility::OnPress);
    assert_eq!(height(&kernel), display, "on a press: all of the display");
    // A policy for another bit (secret) keeps what the screen declared.
    kernel.native_screen_policies(&probe, 1, change(1 << 4));
    assert_eq!(kernel.base_hints(), HintVisibility::Shown);
    assert_eq!(height(&kernel), below_the_row);
    kernel.native_screen_policies(&probe, 1, change(3 << 13));
    assert_eq!(kernel.base_hints(), HintVisibility::Hidden);
    assert_eq!(height(&kernel), display);
}
