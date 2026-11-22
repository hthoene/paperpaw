//! The reader asks `core.device` for the device's memory class to choose how
//! fast it counts a book's pages (`native_index::pace`). The host refuses a
//! reply buffer smaller than the op's `max_reply` with `NoSpace`, so an
//! undersized buffer read every device as small. A test-only plugin asks
//! through the real host as the reader does.
#![cfg(feature = "linked")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture failures fail the test"
)]
#[path = "../../test-support/hosted.rs"]
mod hosted;

use paperpaw_core::native_host::device_service::{DeviceFacts, DeviceHost};
use paperpaw_plugin_reader::native_index;
use paperpaw_platform::Instant;
use paperpaw_sdk::native::{
    self, Error, Host,
    callbacks::DrawFrame,
    declare::{Declarations, EventKind},
    device::DeviceClass,
    events::Incoming,
    reply::Reply,
};
use std::cell::Cell;
use std::rc::Rc;

thread_local! {
    static CLASS: Cell<Option<Option<DeviceClass>>> = const { Cell::new(None) };
}

/// The token of the extra `Started` subscription.
const STARTED: u32 = 7_001;

/// A plugin of the reader's services that only asks, at the start, what the
/// reader asks for the pace of its count (the reader itself takes `Started`
/// for its own token: one subscription of a kind a plugin).
struct Asking;

impl native::Plugin for Asking {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        let mut declarations = Declarations::new(|batch| {
            native::registrations::submit(host, batch).map_err(i32::from)
        });
        declarations
            .subscribe_event(EventKind::Started, STARTED)
            .map_err(|_| Error::Invalid)?
            .submit()
            .map_err(|_| Error::Invalid)?;
        Ok(Self)
    }
    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        _: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        if token == STARTED {
            CLASS.set(Some(native_index::device_class(host)));
        }
        Ok(())
    }
    fn input(
        &mut self,
        _: &Host<'_>,
        _: u32,
        _: native::callbacks::Input<'_>,
    ) -> Result<paperpaw_sdk::Handled, Error> {
        Ok(paperpaw_sdk::Handled::No)
    }
    fn draw(&mut self, _: &Host<'_>, _: u32, _: DrawFrame<'_>) -> Result<(), Error> {
        Ok(())
    }
}

paperpaw_sdk::native_plugin!(
    Asking,
    metadata = include_bytes!("../native/metadata.host.bin"),
    linked = ASKING_ENTRY
);

/// A device of the large memory class.
struct Large;
impl DeviceHost for Large {
    fn device(&self, visit: &mut dyn FnMut(&DeviceFacts<'_>)) -> Result<(), i32> {
        visit(&DeviceFacts {
            id: "large",
            model: "A larger reader",
            catalog: "",
            class: 2,
            caps: 0,
            controls: 0,
            tls_sessions: 0,
            start: 0,
            slept_ms: None,
        });
        Ok(())
    }
}

#[test]
fn the_memory_class_the_device_tells_reaches_the_page_count() {
    let setup = hosted::Setup::large().wired(|w| w.device(Rc::new(Large)));
    let mut reader = hosted::install_in(
        &setup,
        ASKING_ENTRY.entry(),
        "reader",
        &[
            "core.device",
            "core.document",
            "core.files",
            "core.library",
            "core.registry",
            "core.ui",
            "core.work",
        ],
    )
    .expect("the host admits the plugin");
    let _ = reader.kernel().start(Instant(0));
    assert_eq!(CLASS.get(), Some(Some(DeviceClass::Large)));
}
