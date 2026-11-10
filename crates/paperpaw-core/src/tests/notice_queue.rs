//! Notices wait their turn (`docs/abi/ui.md` section 7): one shows at a time,
//! those that come meanwhile wait in order, at most four (two on the X4), the
//! oldest dropped.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]

use super::*;
use paperpaw_platform::MemoryClass;

/// A home screen and a notice screen that logs the text it was made for.
struct Texts(Log);

impl Plugin for Texts {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("nq"), "Notices", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let log = self.0.clone();
        r.add_screen(ScreenSpec::new(
            Id::new_static("nq.base"),
            "base",
            Box::new(move |_, _| Box::new(BaseScreen { log: log.clone() })),
        ))?;
        r.bind_slot(slots::HOME, Id::new_static("nq.base"), Priority::DEFAULT)?;
        let log = self.0.clone();
        r.add_screen(ScreenSpec::new(
            Id::new_static("nq.notice"),
            "notice",
            Box::new(move |_, params| {
                let text = params.get(paperpaw_sdk::builtin::params::TEXT);
                log.borrow_mut().push(String::from(text.unwrap_or("?")));
                Box::new(HalfOverlay)
            }),
        ))?;
        r.bind_slot(
            slots::NOTICE,
            Id::new_static("nq.notice"),
            Priority::DEFAULT,
        )
    }
}

fn kernel(memory: MemoryClass, log: &Log) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE.with_memory(memory))
        .plugin(Texts(log.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    log.borrow_mut().clear();
    kernel
}

/// Lets every notice show in turn: each is over after four seconds.
fn run_out(kernel: &mut Kernel, from: u64) {
    let mut now = from;
    while kernel.notice.is_some() {
        now += 4000;
        kernel.tick(Instant(now));
    }
}

#[test]
fn notices_that_come_while_one_shows_wait_and_show_in_order() {
    let log = log();
    let mut kernel = kernel(MemoryClass::Large, &log);
    for text in ["a", "b", "c"] {
        kernel.apply(Command::Notify(String::from(text)));
        kernel.finish();
    }
    // The first shows; the others wait, not replacing it.
    assert_eq!(*log.borrow(), ["a"]);
    kernel.tick(Instant(4000));
    assert_eq!(*log.borrow(), ["a", "b"]);
    kernel.tick(Instant(8000));
    assert_eq!(*log.borrow(), ["a", "b", "c"]);
    run_out(&mut kernel, 8000);
    assert!(kernel.notice.is_none());
    assert_eq!(log.borrow().len(), 3, "nothing is shown twice");
}

#[test]
fn four_wait_at_most_and_the_oldest_are_dropped() {
    let log = log();
    let mut kernel = kernel(MemoryClass::Large, &log);
    for n in 1..=7 {
        kernel.apply(Command::Notify(alloc::format!("n{n}")));
        kernel.finish();
    }
    run_out(&mut kernel, 0);
    // n1 showed at once; of the six that waited the last four are kept.
    assert_eq!(*log.borrow(), ["n1", "n4", "n5", "n6", "n7"]);
}

#[test]
fn two_wait_at_most_on_the_x4() {
    let log = log();
    let mut kernel = kernel(MemoryClass::Small, &log);
    for n in 1..=5 {
        kernel.apply(Command::Notify(alloc::format!("n{n}")));
        kernel.finish();
    }
    run_out(&mut kernel, 0);
    assert_eq!(*log.borrow(), ["n1", "n4", "n5"]);
}
