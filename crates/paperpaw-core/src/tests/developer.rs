//! The developer link over USB (ADR 0031): reserved in this release, so
//! a plugin that opens it hears at once that it is not there.

use super::*;
use crate::Staging;
use paperpaw_sdk::developer::{self, DevError, DevEvent};

const LINK: Id = Id::new_static("tools.link");
const EVENTS: Id = Id::new_static("tools.link-events");

fn tools(kept: Log) -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    move |r| {
        crate::tests::hear_into(&kept);
        r.on(
            paperpaw_sdk::event::Event::<DevEvent>::new("tools.link-events"),
            |_, p| {
                if let Some(event) = crate::hear::data::<DevEvent>(p) {
                    crate::tests::heard(alloc::format!("{event:?}"));
                }
            },
        )
    }
}

/// What [`tools`] registers under.
fn tools_manifest() -> Manifest {
    Manifest::new(Id::new_static("tools"), "Tools", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::DEVELOPER)
}

#[test]
fn the_developer_link_is_not_there_yet() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .items(tools_manifest(), tools(log.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
        .output
        .commands
        .push(developer::open(LINK, EVENTS).into());
    kernel.run_commands();
    let failed = DevEvent::Failed {
        link: LINK,
        error: DevError::NotAvailable,
    };
    assert_eq!(*log.borrow(), [alloc::format!("{failed:?}")]);
}
