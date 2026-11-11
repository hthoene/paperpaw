//! The hosted kit reports a start-time service call that fails: a test-only
//! plugin (the Files plugin with one more `Started` subscription) opens
//! `core.files` and calls `list` with a reply buffer smaller than the op's
//! `max_reply`, so the call is refused with `NoSpace` as the reader's was on the
//! emulator. The host keeps no report of a callee's own failed event callback
//! (neither `dropped()` nor `warnings()`), so the fixture keeps the status.
#![cfg(feature = "linked")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture failures fail the test"
)]
#[path = "../../test-support/acceptance.rs"]
mod acceptance;
#[path = "../../test-support/hosted.rs"]
mod hosted;

use acceptance::Session;
use paperpaw_native::host_glue::ERR_NO_SPACE;
use paperpaw_plugin_files::native_plugin::NativeFiles;
use paperpaw_sdk::{
    MemoryStorage,
    native::{
        self, Error, Host,
        callbacks::DrawFrame,
        declare::{Declarations, EventKind},
        events::Incoming,
        reply::Reply,
        services::Service,
    },
};
use std::cell::Cell;

// What the extra `Started` callback's service call ended with (the host drops
// a callee's own negative status without a report, so the test keeps it).
thread_local! {
    static LAST: Cell<i32> = const { Cell::new(i32::MAX) };
}

/// The token of the extra `Started` subscription.
const STARTED: u32 = 7_001;

/// Files, but its `Started` callback makes the undersized call.
struct Undersized(NativeFiles);

impl native::Plugin for Undersized {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        let inner = NativeFiles::make(host)?;
        let mut declarations = Declarations::new(|batch| {
            native::registrations::submit(host, batch).map_err(i32::from)
        });
        declarations
            .subscribe_event(EventKind::Started, STARTED)
            .map_err(|_| Error::Invalid)?
            .submit()
            .map_err(|_| Error::Invalid)?;
        Ok(Self(inner))
    }
    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        if token == STARTED {
            let result = Service::open(host, "core.files", 1, 0).and_then(|files| {
                // `list` replies up to 4,096 bytes; 16 cannot hold it.
                let mut short = [0_u8; 16];
                let result = files.call(host, 1, &[], &mut short);
                files.release(host).ok();
                result
            });
            LAST.set(result.map_or_else(i32::from, |_| 0));
            return result.map(drop);
        }
        native::Plugin::event(&mut self.0, host, token, incoming, reply)
    }
    fn input(
        &mut self,
        host: &Host<'_>,
        token: u32,
        input: native::callbacks::Input<'_>,
    ) -> Result<paperpaw_sdk::Handled, Error> {
        native::Plugin::input(&mut self.0, host, token, input)
    }
    fn draw(&mut self, host: &Host<'_>, token: u32, frame: DrawFrame<'_>) -> Result<(), Error> {
        native::Plugin::draw(&mut self.0, host, token, frame)
    }
}

paperpaw_sdk::native_plugin!(
    Undersized,
    metadata = include_bytes!("../native/metadata.host.bin"),
    linked = UNDERSIZED_ENTRY
);

const SERVICES: &[&str] = &["core.device", "core.files", "core.library", "core.ui"];

fn session(wired: bool) -> Session {
    let mut setup = hosted::Setup::x4();
    if wired {
        let card: std::rc::Rc<dyn paperpaw_sdk::Storage> = std::rc::Rc::new(MemoryStorage::new());
        setup = setup.wired(move |w| {
            w.documents(paperpaw_sdk::MemoryClass::Small, 800)
                .files(card.clone())
        });
    }
    Session::start(setup, UNDERSIZED_ENTRY.entry(), "files", SERVICES).unwrap()
}

#[test]
fn a_start_call_with_a_buffer_below_max_reply_is_refused_with_no_space() {
    let mut session = session(true);
    session.settle();
    assert_eq!(LAST.get(), ERR_NO_SPACE);
}

#[test]
fn a_service_that_is_not_wired_is_refused_at_open() {
    // No `Setup::wired`: the family is absent and the open is refused.
    let mut session = session(false);
    session.settle();
    assert!(LAST.get() < 0);
}
