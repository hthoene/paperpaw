//! The off screen (the slot `core.off`): drawn for a device the shell
//! switches off by itself, told why, whole, and whichever screen the slot is
//! bound to; nothing where no screen is.

use super::*;
use crate::Staging;
use paperpaw_sdk::builtin::params;

const DEFAULT: Id = Id::new_static("off.default");
const OTHER: Id = Id::new_static("off.other");

/// A screen that says with which reason it was opened.
struct Off;

impl crate::testing::Fake for Off {}

fn off_plugin(kept: Log, bound: bool) -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    move |r| {
        for id in [DEFAULT, OTHER] {
            let log = kept.clone();
            r.add_fake(crate::testing::FakeSpec::new(
                id.clone(),
                "off",
                Box::new(move |_, given| {
                    let reason = given.get(params::OFF_REASON).unwrap_or("none");
                    log.borrow_mut()
                        .push(alloc::format!("{id} opened with {reason}"));
                    Box::new(Off)
                }),
            ))?;
        }
        if bound {
            r.bind_slot(slots::OFF, DEFAULT, Priority::DEFAULT)?;
        }
        Ok(())
    }
}

fn kernel(log: &Log, bound: bool) -> Kernel {
    let manifest = Manifest::new(Id::new_static("off"), "Off", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING);
    let mut kernel = Kernel::builder(DEVICE)
        .items(manifest, off_plugin(log.clone(), bound))
        .storage(paperpaw_sdk::MemoryStorage::new())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    log.borrow_mut().clear();
    kernel
}

fn top(kernel: &Kernel) -> String {
    let env = kernel.shared.place(Instant(0));
    String::from(env.stack().last().unwrap().screen.as_str())
}

#[test]
fn the_off_screen_is_told_why_and_drawn_whole() {
    let log = log();
    let mut kernel = kernel(&log, true);
    let effects = kernel
        .switch_off_screen(Instant(5), params::OFF_TIMER)
        .expect("a screen is bound to the slot");
    assert_eq!(top(&kernel), "off.default");
    assert_eq!(*log.borrow(), ["off.default opened with timer"]);
    assert!(
        matches!(
            effects.refresh(),
            Some((_, paperpaw_sdk::RefreshMode::Full))
        ),
        "{:?}",
        effects.refresh()
    );
    // Nothing switches off by itself: the shell does, after the frame.
    assert!(!effects.power_off() && !effects.sleep());
}

#[test]
fn a_plugin_can_take_the_off_screen() {
    let log = log();
    let mut kernel = kernel(&log, true);
    kernel.apply(Ask::BindSlot(slots::OFF, Some(OTHER)));
    kernel
        .switch_off_screen(Instant(5), params::OFF_TIMER)
        .unwrap();
    assert_eq!(top(&kernel), "off.other");
    assert_eq!(*log.borrow(), ["off.other opened with timer"]);
}

#[test]
fn without_a_screen_for_the_slot_nothing_is_drawn() {
    let log = log();
    let mut kernel = kernel(&log, false);
    assert!(
        kernel
            .switch_off_screen(Instant(5), params::OFF_TIMER)
            .is_none()
    );
    assert!(log.borrow().is_empty());
}
