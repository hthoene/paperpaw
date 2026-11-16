//! Real SDK calls through the reused backing adapter, with deterministic services.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test assertions"
)]
extern crate alloc;
use std::{cell::RefCell, rc::Rc};

use paperpaw_core::native_host::registrations::{
    EventStarted, Record, ServiceDocAt, ServiceDocSection, ServiceDocState, ServiceDocXpointer,
    ServiceEnd, ServiceLibKey, records_decode,
};
use paperpaw_scripted_host::{ScriptedHost, event_batch, row};
use paperpaw_sdk::native::{Error, Host, Plugin, events::Incoming, reply::Reply};

#[path = "../src/native_backing.rs"]
mod backing;

#[derive(Default)]
struct Probe;
impl Plugin for Probe {
    fn make(_: &Host<'_>) -> Result<Self, Error> {
        Ok(Self)
    }
    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        _: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let backing = backing::Backing::open(host)?;
        match token {
            1 if backing.can_send() => {
                if let Some(key) = backing.digest(host, "book.epub")? {
                    reply.write(key.as_bytes())?;
                }
            }
            2 => {
                if let Some(pointer) = backing.xpointer(host, "book.epub", (2, 7))? {
                    reply.write(pointer.as_bytes())?;
                }
            }
            _ => {
                let pointer = if token == 3 { "/known" } else { "" };
                if let Some((section, offset)) = backing.locate(host, "book.epub", pointer, 0.5)? {
                    let mut bytes = [0; 8];
                    bytes[..4].copy_from_slice(&section.to_le_bytes());
                    bytes[4..].copy_from_slice(&offset.to_le_bytes());
                    reply.write(&bytes)?;
                }
            }
        }
        Ok(())
    }
}
paperpaw_sdk::native_plugin!(
    Probe,
    metadata = include_bytes!("../native/metadata.host.bin"),
    linked = ENTRY
);

fn run(host: &ScriptedHost, token: u32) -> (i32, Vec<u8>) {
    let plugin = host.load(ENTRY.entry()).unwrap();
    let batch = event_batch(&Record::EventStarted(EventStarted { now_ms: 0 })).unwrap();
    let (done, reply) = plugin.stage_call(token, &batch, 128).unwrap();
    (done.status, reply)
}

fn document_service(
    host: &ScriptedHost,
    pointers: bool,
    locate_error: i32,
) -> Rc<RefCell<Vec<u32>>> {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let log = calls.clone();
    host.service("core.document", 1, 0, move |op, input| {
        log.borrow_mut().push(op);
        let records: Vec<_> = records_decode(input).unwrap().iter().collect();
        match op {
            1 => Ok(row(&Record::ServiceDocState(ServiceDocState {
                document: 4, state: 0, progress_ppm: 0, status: 0,
                flags: if pointers { 4 } else { 0 }, conversion: 7, format: "format.epub",
            })).unwrap()),
            18 => Ok(Vec::new()),
            5 => {
                let mut bytes = Vec::new();
                for (index, length) in [100, 0, 300].into_iter().enumerate() {
                    bytes.extend(row(&Record::ServiceDocSection(ServiceDocSection {
                        index: u32::try_from(index).unwrap(), length, flags: 0,
                    })).unwrap());
                }
                bytes.extend(row(&Record::ServiceEnd(ServiceEnd {
                    flags: 1, total: 3, generation: 1, next: 3, cursor: None,
                })).unwrap());
                Ok(bytes)
            }
            10 => {
                assert!(matches!(&records[..], [Record::ServiceDocAt(at)] if at.document == 4 && at.section == 2 && at.offset == 7));
                Ok(row(&Record::ServiceDocXpointer(ServiceDocXpointer { document: 4, text: "/known" })).unwrap())
            }
            11 => {
                if locate_error != 0 { return Err(locate_error); }
                Ok(row(&Record::ServiceDocAt(ServiceDocAt {
                    document: 4, section: 2, offset: 77, insert: 0, cursor: 0, generation: 0,
                })).unwrap())
            }
            _ => Err(-1),
        }
    });
    calls
}

#[test]
fn fingerprint_comes_from_library_not_the_path_or_its_filename() {
    let host = ScriptedHost::new();
    host.service("core.library", 1, 0, |op, input| {
        assert_eq!(op, 25);
        let records: Vec<_> = records_decode(input).unwrap().iter().collect();
        assert!(matches!(&records[..], [Record::ServiceLibRef(book)] if book.path == "book.epub"));
        Ok(row(&Record::ServiceLibKey(ServiceLibKey {
            key: "0123456789abcdef",
            fingerprint: "d41d8cd98f00b204e9800998ecf8427e",
            size: 0,
        }))
        .unwrap())
    });
    assert_eq!(
        run(&host, 1),
        (0, b"d41d8cd98f00b204e9800998ecf8427e".to_vec())
    );
}

#[test]
fn xpointer_and_locate_use_the_document_and_close_it() {
    for (token, expected, ops) in [
        (2, b"/known".to_vec(), vec![1, 10, 18]),
        (
            3,
            [2u32.to_le_bytes(), 77u32.to_le_bytes()].concat(),
            vec![1, 11, 18],
        ),
    ] {
        let host = ScriptedHost::new();
        let calls = document_service(&host, true, 0);
        assert_eq!(run(&host, token), (0, expected));
        assert_eq!(*calls.borrow(), ops);
    }
}

#[test]
fn a_format_without_xpointers_uses_percent_and_closes_the_document() {
    let host = ScriptedHost::new();
    let calls = document_service(&host, false, 0);
    assert_eq!(
        run(&host, 4),
        (0, [2u32.to_le_bytes(), 100u32.to_le_bytes()].concat())
    );
    assert_eq!(*calls.borrow(), [1, 5, 18]);
}

#[test]
fn an_unknown_pointer_falls_back_but_busy_does_not_and_both_close() {
    for (error, status, expected_ops) in [(-10, 0, vec![1, 11, 5, 18]), (-9, -9, vec![1, 11, 18])] {
        let host = ScriptedHost::new();
        let calls = document_service(&host, true, error);
        let (got, _) = run(&host, 3);
        assert_eq!(got, status);
        assert_eq!(*calls.borrow(), expected_ops);
    }
}
