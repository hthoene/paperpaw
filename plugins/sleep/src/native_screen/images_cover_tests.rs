//! The cover of the book read last, over a scripted `core.library` (#346: the
//! host answered busy for the screen-sized cover and the screen showed the cat).
#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use paperpaw_core::native_host::registrations::{
    EventLibraryChanged, Record, ServiceDocImage, ServiceEnd, ServiceLibEntry,
};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NOT_FOUND};
use paperpaw_scripted_host::{ScriptedHost, event_batch, row};
use paperpaw_sdk::native::{Plugin, events::Incoming, reply::Reply};
use std::{cell::RefCell, rc::Rc, vec, vec::Vec};

/// Asks for the cover of the book read last at the screen's size, and says
/// what came of it: 0 none, 1 a cover, 2 busy.
#[derive(Default)]
struct Probe;

impl Plugin for Probe {
    fn make(_: &Host<'_>) -> Result<Self, Error> {
        Ok(Self)
    }
    fn event(
        &mut self,
        host: &Host<'_>,
        _: u32,
        _: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let said = match last_cover(host, 800)? {
            Found::None => 0,
            Found::Cover(_) => 1,
            Found::Busy => 2,
        };
        reply.write(&[said])
    }
}
paperpaw_sdk::native_plugin!(
    Probe,
    metadata = include_bytes!("../../native/metadata.host.bin"),
    linked = PROBE
);

/// What `core.library` answers, call after call, to the cover of the book
/// read last; the asks' sides are kept.
fn library(host: &ScriptedHost, covers: Vec<Result<(), i32>>) -> Rc<RefCell<Vec<usize>>> {
    let asked = Rc::new(RefCell::new(Vec::new()));
    let (log, mut covers) = (asked.clone(), covers.into_iter());
    host.service("core.library", 1, 0, move |op, _| match op {
        // The query: one book, opened.
        2 => Ok([
            row(&Record::ServiceLibEntry(ServiceLibEntry {
                key: 1,
                title: "Byttet",
                author: "",
                series: "",
                number: "",
                progress: 1,
                flags: 2,
                added: 0,
                badges: "",
            }))?,
            row(&Record::ServiceEnd(ServiceEnd {
                flags: 1,
                total: 1,
                generation: 1,
                next: 1,
                cursor: None,
            }))?,
        ]
        .concat()),
        7 => {
            let at = log.borrow().len();
            log.borrow_mut().push(at);
            match covers.next().unwrap_or(Err(ERR_NOT_FOUND)) {
                Ok(()) => row(&Record::ServiceDocImage(ServiceDocImage {
                    image: 3,
                    index: u32::MAX,
                    width: 300,
                    height: 450,
                    flags: 0,
                })),
                Err(status) => Err(status),
            }
        }
        _ => Ok(Vec::new()),
    });
    asked
}

fn said(plugin: &paperpaw_scripted_host::Plugin<'_>) -> u8 {
    let event = event_batch(&Record::EventLibraryChanged(EventLibraryChanged {
        now_ms: 0,
        what: 0,
        generation: 1,
        path: "",
    }))
    .unwrap();
    let (done, said) = plugin.stage_call(1, &event, 16).unwrap();
    assert_eq!(done.status, 0);
    said[0]
}

#[test]
fn the_cover_at_the_screens_size_is_the_one_drawn() {
    let host = ScriptedHost::new();
    let asked = library(&host, vec![Ok(())]);
    let plugin = host.load(PROBE.entry()).unwrap();
    assert_eq!(said(&plugin), 1);
    assert_eq!(asked.borrow().len(), 1, "the first ask is enough");
}

#[test]
fn a_cover_not_made_at_the_screens_size_yet_is_the_lists_one() {
    let host = ScriptedHost::new();
    let asked = library(&host, vec![Err(ERR_BUSY), Ok(())]);
    let plugin = host.load(PROBE.entry()).unwrap();
    assert_eq!(said(&plugin), 1, "the cat showed where the cover was busy");
    assert_eq!(asked.borrow().len(), 2);
}

#[test]
fn a_cover_busy_at_both_sizes_is_waited_for_and_none_is_none() {
    let host = ScriptedHost::new();
    library(&host, vec![Err(ERR_BUSY), Err(ERR_BUSY)]);
    let plugin = host.load(PROBE.entry()).unwrap();
    assert_eq!(said(&plugin), 2);
    let host = ScriptedHost::new();
    library(&host, vec![Err(ERR_NOT_FOUND), Err(ERR_NOT_FOUND)]);
    let plugin = host.load(PROBE.entry()).unwrap();
    assert_eq!(said(&plugin), 0);
}
