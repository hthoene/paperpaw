//! `event.library_changed` from the library host to the plugins that
//! subscribed: one event for the changes raised since the last pass, the
//! changer hears it too, and a plugin that did not subscribe hears nothing.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use crate::native_host::{
    codec,
    commands::CommandStage,
    lib_changed::{ANNOTATIONS, STATUS},
    lib_host::LibraryHost,
    lib_reads::tests::card,
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
    registrations as wire,
};
use alloc::{rc::Rc, string::String, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_INVALID, OK};
use paperpaw_sdk::Id;

/// What one plugin was told: `what`, `generation`, `path`.
type Told = Rc<RefCell<Vec<(u32, u32, String)>>>;

struct Probe {
    subscribed: bool,
    told: Told,
}

impl NativeInstance for Probe {
    fn running(&self) -> bool {
        true
    }
    fn stop(&self) {}
    fn subscribed(&self, tag: u32) -> bool {
        self.subscribed && tag == wire::EVENT_LIBRARY_CHANGED_TAG
    }
    fn service_event(&self, bytes: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        let wire::Record::EventLibraryChanged(event) = codec::decode_any_single(bytes).unwrap()
        else {
            return Err(ERR_INVALID);
        };
        self.told
            .borrow_mut()
            .push((event.what, event.generation, event.path.into()));
        Ok(OK)
    }
    fn service_event_staged(
        &self,
        bytes: &[u8],
        env: &paperpaw_sdk::Env<'_>,
        _: &mut CommandStage<'_>,
    ) -> Result<i32, i32> {
        self.service_event(bytes, env)
    }
}

impl Dispatch for Probe {
    fn event(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn input(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn draw(
        &self,
        _: u32,
        _: &wire::DrawFrame<'_>,
        _: &paperpaw_sdk::Env<'_>,
        _: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn dropped(&self, _: u32, _: i32) {}
}

struct World {
    kernel: crate::Kernel,
    host: Rc<LibraryHost>,
    heard: Told,
    silent: Told,
}

fn world() -> World {
    let card = card();
    let host = Rc::new(LibraryHost::new(
        card.storage.clone(),
        card.env.clone(),
        None,
    ));
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let (heard, silent) = (Told::default(), Told::default());
    for (name, subscribed, told) in [("a", true, &heard), ("b", false, &silent)] {
        let probe = Probe {
            subscribed,
            told: told.clone(),
        };
        kernel
            .shared
            .native_instances
            .test_insert(
                Id::try_parse(name).unwrap(),
                Some(NativeLease::try_new(probe).unwrap()),
                true,
            )
            .unwrap();
    }
    kernel
        .shared
        .native_instances
        .test_resources()
        .unwrap()
        .hosts()
        .wire_library(host.clone())
        .unwrap();
    World {
        kernel,
        host,
        heard,
        silent,
    }
}

/// One pass of the service pump, as the kernel runs it after a call.
fn pump(w: &mut World) {
    w.kernel.native_service_pump();
}

#[test]
fn a_raised_change_reaches_the_subscriber_once_with_the_book_and_the_generation() {
    let mut w = world();
    w.host.raise_changed(ANNOTATIONS, "books/a.epub");
    pump(&mut w);
    let told = w.heard.borrow().clone();
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].0, ANNOTATIONS);
    assert_ne!(told[0].1, 0, "the catalog's generation, never 0");
    assert_eq!(told[0].2, "books/a.epub");
    // Told once: a pass with nothing raised tells nothing.
    pump(&mut w);
    assert_eq!(w.heard.borrow().len(), 1);
}

#[test]
fn a_plugin_that_did_not_subscribe_hears_nothing() {
    let mut w = world();
    w.host.raise_changed(ANNOTATIONS, "books/a.epub");
    pump(&mut w);
    assert!(w.silent.borrow().is_empty());
    assert_eq!(w.heard.borrow().len(), 1);
}

#[test]
fn changes_raised_before_a_pass_are_one_event_and_two_books_name_none() {
    let mut w = world();
    w.host.raise_changed(ANNOTATIONS, "books/a.epub");
    w.host.raise_changed(STATUS, "books/b.epub");
    pump(&mut w);
    let told = w.heard.borrow().clone();
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].0, ANNOTATIONS | STATUS);
    assert_eq!(told[0].2, "");
}
