//! The sleep screen shown again while asleep (`command.show_again`): the plan
//! the kernel keeps, when the shell is told it is due, what it draws, and when
//! it ends.

use super::*;
use crate::Staging;

const SCREEN: Id = Id::new_static("again.screen");

struct Again;

impl crate::testing::Fake for Again {}

fn kernel() -> Kernel {
    let manifest = Manifest::new(Id::new_static("again"), "Again", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING);
    let mut kernel = Kernel::builder(DEVICE)
        .items(manifest, |r: &mut Staging| {
            r.add_fake(crate::testing::FakeSpec::new(
                SCREEN,
                "again",
                Box::new(|_, _| Box::new(Again)),
            ))
        })
        .storage(paperpaw_sdk::MemoryStorage::new())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.apply(Ask::OpenScreen(SCREEN, paperpaw_sdk::Params::new()));
    kernel
}

fn screen(kernel: &Kernel) -> paperpaw_sdk::ScreenKey {
    paperpaw_sdk::ScreenKey::new(kernel.stack.last().unwrap().serial)
}

fn ask(kernel: &mut Kernel, after_ms: u32, every_ms: u32) {
    let screen = screen(kernel);
    kernel.apply(Ask::ShowAgain {
        screen,
        after_ms,
        every_ms,
    });
}

#[test]
fn it_is_due_at_its_time_draws_the_screen_partly_and_repeats_from_the_time_asked_for() {
    let mut kernel = kernel();
    kernel.suspend(Instant(1_000));
    ask(&mut kernel, 900_000, 900_000);
    assert_eq!(kernel.show_again_at(), Some(Instant(901_000)));
    assert!(kernel.show_again_now(Instant(900_999)).is_none());
    let effects = kernel.show_again_now(Instant(901_000)).expect("due");
    assert!(matches!(
        effects.refresh(),
        Some((_, paperpaw_sdk::RefreshMode::Partial))
    ));
    // The next from the time asked for, not from when it came.
    assert_eq!(kernel.show_again_at(), Some(Instant(1_801_000)));
    // A clock that ran far ahead (a long nap): one show, the next after now.
    assert!(kernel.show_again_now(Instant(5_000_000)).is_some());
    assert_eq!(kernel.show_again_at(), Some(Instant(5_401_000)));
}

#[test]
fn once_is_once_and_a_cancel_ends_it() {
    let mut kernel = kernel();
    kernel.suspend(Instant(0));
    ask(&mut kernel, 60_000, 0);
    assert!(kernel.show_again_now(Instant(60_000)).is_some());
    assert_eq!(kernel.show_again_at(), None);
    ask(&mut kernel, 60_000, 60_000);
    ask(&mut kernel, 0, 0);
    assert_eq!(kernel.show_again_at(), None);
}

#[test]
fn nothing_is_planned_awake_and_the_wake_ends_the_plan() {
    let mut kernel = kernel();
    ask(&mut kernel, 60_000, 0);
    assert_eq!(kernel.show_again_at(), None, "awake");
    kernel.suspend(Instant(0));
    ask(&mut kernel, 60_000, 0);
    assert!(kernel.show_again_at().is_some());
    kernel.resume(Instant(10));
    assert_eq!(kernel.show_again_at(), None);
}

#[test]
fn a_screen_that_is_gone_is_not_drawn_and_the_plan_ends() {
    let mut kernel = kernel();
    kernel.suspend(Instant(0));
    ask(&mut kernel, 60_000, 60_000);
    kernel.apply(Ask::Close(screen(&kernel)));
    assert!(kernel.show_again_now(Instant(60_000)).is_none());
    assert_eq!(kernel.show_again_at(), None);
}
