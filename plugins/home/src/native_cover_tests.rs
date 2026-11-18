#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use crate::native_plugin::NativeHome;
use crate::native_recent::{Book, Covered, cover_known, with_library};
use paperpaw_core::native_host::registrations::{
    EventLibraryChanged, EventWidgetMade, Record, ServiceDocImage,
};
use paperpaw_scripted_host::{ScriptedHost, event_batch, row};
use paperpaw_sdk::native::{Plugin, events::Incoming};
use std::{cell::Cell, rc::Rc};

#[derive(Default)]
struct Probe(NativeHome);

fn book() -> Book {
    Book {
        key: 1,
        generation: 1,
        title: String::new(),
        progress: 0,
    }
}

impl Plugin for Probe {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        NativeHome::make(host).map(Self)
    }
    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let token = if token == 100 {
            crate::declarations::CARD
        } else {
            token
        };
        self.0.event(host, token, incoming, reply)?;
        let widgets = self.0.test_widgets();
        let (mine, listing) = (&widgets.items[0].mine, &widgets.listing);
        let cover = with_library(host, |lib| cover_known(host, listing, lib, &book()))?;
        mine.waiting.set(cover == Covered::Busy);
        reply.write(&[u8::from(matches!(cover, Covered::Ready(_)))])
    }
}
paperpaw_sdk::native_plugin!(
    Probe,
    metadata = include_bytes!("../native/metadata.host.bin"),
    linked = PROBE
);

#[test]
fn a_ready_cover_is_shown_after_library_changed_without_a_minute_tick() {
    let host = ScriptedHost::new();
    let _ui = host.dialogs();
    let ready = Rc::new(Cell::new(false));
    let calls = Rc::new(Cell::new(0));
    let (available, asked) = (ready.clone(), calls.clone());
    host.service("core.library", 1, 0, move |op, _| {
        assert_eq!(op, 7);
        asked.set(asked.get() + 1);
        if !available.get() {
            return Err(-9);
        }
        row(&Record::ServiceDocImage(ServiceDocImage {
            image: 3,
            index: u32::MAX,
            width: 100,
            height: 150,
            flags: 0,
        }))
    });
    let plugin = host.load(PROBE.entry()).unwrap();
    let start = event_batch(&Record::EventWidgetMade(EventWidgetMade {
        now_ms: 0,
        widget: "home.continue",
        instance: 1,
        place: "home.pages",
    }))
    .unwrap();
    let (done, shown) = plugin.stage_call(100, &start, 16).unwrap();
    assert_eq!(done.status, 0);
    assert_eq!(shown, [0]);
    assert_eq!(calls.get(), 1);
    ready.set(true);
    let changed = |what| {
        event_batch(&Record::EventLibraryChanged(EventLibraryChanged {
            now_ms: 100,
            what,
            generation: 1,
            path: "",
        }))
        .unwrap()
    };
    let (done, shown) = plugin
        .stage_call(crate::declarations::LIBRARY_CHANGED, &changed(1), 16)
        .unwrap();
    assert_eq!(done.status, 0);
    assert_eq!(
        shown,
        [0],
        "an unrelated library change must leave the waiting cover alone"
    );
    assert_eq!(calls.get(), 1);
    let (done, shown) = plugin
        .stage_call(crate::declarations::LIBRARY_CHANGED, &changed(1 << 6), 16)
        .unwrap();
    assert_eq!(done.status, 0);
    assert_eq!(
        shown,
        [1],
        "the cover-ready event must replace the cached Busy result"
    );
    assert_eq!(calls.get(), 2);
    let (done, shown) = plugin
        .stage_call(crate::declarations::LIBRARY_CHANGED, &changed(1 << 6), 16)
        .unwrap();
    assert_eq!(done.status, 0);
    assert_eq!(shown, [1]);
    assert_eq!(
        calls.get(),
        2,
        "a cover already ready is not requested again"
    );
}

/// What the host answers for a cover that is not about the book (no room for
/// its work, a list older than the catalog because the book was read since)
/// leaves the card drawn without it and says nothing of the book: the cover is
/// asked for again and shown when the host has it (#308, Byttet's cover
/// missing for good after its progress changed).
fn a_cover_refused_for_now_is_asked_for_again(status: i32) {
    let host = ScriptedHost::new();
    let _ui = host.dialogs();
    let ready = Rc::new(Cell::new(false));
    let calls = Rc::new(Cell::new(0));
    let (available, asked) = (ready.clone(), calls.clone());
    host.service("core.library", 1, 0, move |op, _| {
        assert_eq!(op, 7);
        asked.set(asked.get() + 1);
        if !available.get() {
            return Err(status);
        }
        row(&Record::ServiceDocImage(ServiceDocImage {
            image: 3,
            index: u32::MAX,
            width: 100,
            height: 150,
            flags: 0,
        }))
    });
    let plugin = host.load(PROBE.entry()).unwrap();
    let start = event_batch(&Record::EventWidgetMade(EventWidgetMade {
        now_ms: 0,
        widget: "home.continue",
        instance: 1,
        place: "home.pages",
    }))
    .unwrap();
    let (done, shown) = plugin.stage_call(100, &start, 16).unwrap();
    assert_eq!(done.status, 0, "the refusal must not fail the call");
    assert_eq!(shown, [0]);
    assert_eq!(calls.get(), 1);
    ready.set(true);
    let ended = event_batch(&Record::EventLibraryChanged(EventLibraryChanged {
        now_ms: 100,
        what: 1 << 6,
        generation: 2,
        path: "",
    }))
    .unwrap();
    let (done, shown) = plugin
        .stage_call(crate::declarations::LIBRARY_CHANGED, &ended, 16)
        .unwrap();
    assert_eq!(done.status, 0);
    assert_eq!(shown, [1], "the cover is asked for again, not kept as none");
    assert_eq!(calls.get(), 2);
}

#[test]
fn a_cover_of_a_list_older_than_the_catalog_is_asked_for_again() {
    a_cover_refused_for_now_is_asked_for_again(-6);
}

#[test]
fn a_cover_refused_for_lack_of_memory_is_asked_for_again() {
    a_cover_refused_for_now_is_asked_for_again(-4);
}
