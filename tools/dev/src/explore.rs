//! A small, bounded exploration (`paperpaw-dev explore`): every target of
//! every screen of the plugins looked at, once, each from a fresh start,
//! as a cheap baseline beside written flows.
//!
//! For each screen (opened as `perf` opens it), each part that can be
//! used (a row, a button, a switch, a slider, a box, a link) is used
//! once: tapped on a touch screen, else reached with *Down* and pressed
//! with *Select*. Then the home screen must be reached again (*Home*,
//! else *Back* and *Home*). Reported:
//!
//! - `explore.failed`, error: using it panicked, an allocation was
//!   refused (the device would have run out of memory), or the home
//!   screen could not be reached after.
//! - `explore.done`, note: how many targets on how many screens; where
//!   the bounds ([`TARGETS`] a screen, [`RUNS`] a profile) left some out.

use std::panic::{AssertUnwindSafe, catch_unwind};

use paperpaw_core::sdk::builtin::{actions, params};
use paperpaw_core::sdk::semantics::{Part, Role};
use paperpaw_emulator::{budget, headless};

use crate::cases;
use crate::cli::Context;
use crate::report::{Finding, Report};
use crate::session::{self, Session};
use crate::variant::Variant;

/// Targets of one screen used at most.
pub const TARGETS: usize = 20;

/// Targets used at most on one profile.
pub const RUNS: usize = 200;

/// Whether a part can be used.
fn usable(part: &Part) -> bool {
    matches!(
        part.role,
        Role::Row
            | Role::Button
            | Role::Switch
            | Role::Slider
            | Role::Check
            | Role::Radio
            | Role::Link
    )
}

/// A fresh session without first-time hints (they would open over a
/// screen when it shows).
fn fresh(context: &Context, variant: &Variant) -> Result<Session, String> {
    let start = session::Start {
        settings: vec![("help.show".into(), "false".into())],
        ..session::Start::default()
    };
    Session::start(context, variant, &start)
}

/// Opens `screen` as `perf` does: as it is, else with the sample book.
fn open(session: &mut Session, screen: &str) -> bool {
    let book = [(params::PATH, session::BOOK)];
    for given in [&[][..], &book[..]] {
        if session.open(screen, given).is_ok() && session.screen().as_deref() == Some(screen) {
            return true;
        }
    }
    false
}

pub fn run(context: &Context, report: &mut Report) -> Result<(), String> {
    for variant in context.variants_per_profile() {
        let screens = cases::screens(&Session::new(context, &variant)?, &context.target);
        let (mut runs, mut used, mut left) = (0, 0, 0);
        for screen in &screens {
            let targets: Vec<Part> = {
                let mut session = fresh(context, &variant)?;
                if !open(&mut session, screen) {
                    continue;
                }
                session.parts().into_iter().filter(usable).collect()
            };
            for (index, target) in targets.iter().enumerate() {
                if index >= TARGETS || runs >= RUNS {
                    left += 1;
                    continue;
                }
                runs += 1;
                match try_target(context, &variant, screen, target, index) {
                    Ok(()) => used += 1,
                    Err(what) => report.add(
                        Finding::error(
                            "explore.failed",
                            screen.clone(),
                            format!("Using \"{}\": {what}.", target.label),
                        )
                        .variant(variant.name()),
                    ),
                }
            }
        }
        let rest = if left > 0 {
            format!(" ({left} more left out by the bounds)")
        } else {
            String::new()
        };
        report.add(
            Finding::info(
                "explore.done",
                "screens",
                format!(
                    "{used} of {runs} targets used without trouble on {} screens{rest}.",
                    screens.len()
                ),
            )
            .variant(variant.name()),
        );
    }
    Ok(())
}

/// Uses the target `index` of `screen`, from a fresh start; what went
/// wrong, if anything.
fn try_target(
    context: &Context,
    variant: &Variant,
    screen: &str,
    target: &Part,
    index: usize,
) -> Result<(), String> {
    let refused = budget::stats().failed;
    let touch = variant.profile.device.input.touch;
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<(), String> {
        let mut session = fresh(context, variant)?;
        if !open(&mut session, screen) {
            return Ok(());
        }
        if touch {
            let a = target.area;
            let tap = format!(
                "tap:{}:{}",
                i32::from(a.x) + i32::from(a.width) / 2,
                i32::from(a.y) + i32::from(a.height) / 2
            );
            headless::run_script(&mut session.device, &tap)?;
        } else {
            // The focus walked down to it, as far as it goes.
            for _ in 0..index {
                let _ = headless::run_script(&mut session.device, "down");
            }
            let _ = session.run(actions::CONFIRM.as_str());
        }
        session.settle();
        session.go_home()
    }));
    let result = match result {
        Ok(result) => result,
        Err(panic) => Err(format!(
            "it panicked: {}",
            panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(ToString::to_string))
                .unwrap_or_else(|| String::from("a panic"))
        )),
    };
    result?;
    let now = budget::stats().failed;
    if now > refused {
        return Err(format!(
            "{} allocations were refused: the device would have run out of memory",
            now - refused
        ));
    }
    Ok(())
}
