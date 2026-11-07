#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::*;
use core::{cell::Cell, ffi::c_void, ptr};
use paperpaw_native::host_glue as abi;

struct Fake {
    calls: Cell<u32>,
    error: i32,
    malformed: u8,
}
unsafe extern "C" fn run(
    ctx: *mut c_void,
    id: PpBytes,
    cursor: u32,
    input: PpBytes,
    out: PpBuffer,
) -> i32 {
    // SAFETY: this fixture installs a Fake context and receives live wrapper slices.
    let (fake, id, input, out) = unsafe {
        (
            &*ctx.cast::<Fake>(),
            core::slice::from_raw_parts(id.ptr, id.len),
            core::slice::from_raw_parts(input.ptr, input.len),
            core::slice::from_raw_parts_mut(out.ptr, out.cap),
        )
    };
    fake.calls.set(fake.calls.get() + 1);
    assert_eq!(id, b"reader.insert");
    assert_eq!(input, b"input");
    if fake.error != 0 {
        return fake.error;
    }
    if fake.malformed == 1 {
        return out.len() as i32 + 1;
    }
    if cursor >= 3 {
        return abi::ERR_NOT_FOUND;
    }
    let status = [7, abi::ERR_CANCELLED, abi::ERR_INVALID][cursor as usize];
    let reply = Record::StageReply(wire::StageReply {
        subscriber: "aids",
        index: cursor + u32::from(fake.malformed == 2),
        status,
        flags: [1, 2, 3][cursor as usize],
        signature: 123,
        payload: if status >= 0 || fake.malformed == 3 {
            b"opaque"
        } else {
            b""
        },
    });
    wire::record_encode_one(&reply, out).map_or_else(|e| e, |n| n as i32)
}

#[test]
fn lending_cursor_exposes_status_grants_and_opaque_payload_then_fuses() {
    let mut table = abi::host_table();
    table.stage_run = run;
    let mut fake = Fake {
        calls: Cell::new(0),
        error: 0,
        malformed: 0,
    };
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut fake).cast(),
    };
    let stage = Stage {
        host: &host,
        id: "reader.insert",
    };
    let mut out = [0; 200];
    let mut cursor = stage.run(b"input", &mut out);
    for (index, expected, grant) in [
        (0, Ok(()), true),
        (1, Err(Error::Cancelled), false),
        (2, Err(Error::Invalid), true),
    ] {
        let reply = cursor.next().unwrap().unwrap();
        assert_eq!(reply.index(), index);
        assert_eq!(reply.subscriber(), "aids");
        assert_eq!(reply.signature(), 123);
        assert_eq!(reply.status(), expected);
        assert_eq!(reply.style_granted(), grant);
        assert_eq!(
            reply.payload(),
            if index == 0 { &b"opaque"[..] } else { &[] }
        );
    }
    assert!(cursor.next().unwrap().is_none());
    assert!(cursor.next().unwrap().is_none());
    assert_eq!(fake.calls.get(), 4);
}

#[test]
fn refusals_and_malformed_envelopes_fuse_without_retrying() {
    for (error, malformed, expected) in [
        (abi::ERR_DENIED, 0, Error::Denied),
        (abi::ERR_STATE, 0, Error::State),
        (abi::ERR_NO_SPACE, 0, Error::NoSpace),
        (abi::ERR_INVALID, 0, Error::Invalid),
        (0, 1, Error::Invalid),
        (0, 2, Error::Invalid),
    ] {
        let mut table = abi::host_table();
        table.stage_run = run;
        let mut fake = Fake {
            calls: Cell::new(0),
            error,
            malformed,
        };
        let host = Host {
            table: &table,
            ctx: ptr::from_mut(&mut fake).cast(),
        };
        let stage = Stage {
            host: &host,
            id: "reader.insert",
        };
        let mut out = [0; 200];
        let mut cursor = stage.run(b"input", &mut out);
        assert_eq!(cursor.next().unwrap_err(), expected);
        assert!(cursor.next().unwrap().is_none());
        assert_eq!(fake.calls.get(), 1);
    }
}

#[test]
fn failed_subscriber_with_nonempty_payload_is_invalid_and_fused() {
    let mut table = abi::host_table();
    table.stage_run = run;
    let mut fake = Fake {
        calls: Cell::new(0),
        error: 0,
        malformed: 3,
    };
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut fake).cast(),
    };
    let stage = Stage {
        host: &host,
        id: "reader.insert",
    };
    let mut out = [0; 200];
    let mut cursor = stage.run(b"input", &mut out);
    assert!(cursor.next().unwrap().is_some());
    assert_eq!(cursor.next().unwrap_err(), Error::Invalid);
    assert!(cursor.next().unwrap().is_none());
}

#[test]
fn reply_capacity_matches_generated_framing_at_every_declared_size() {
    let subscriber = "s".repeat(64);
    let payload = [0; 3840];
    for max in 1..=3840 {
        let record = Record::StageReply(wire::StageReply {
            subscriber: &subscriber,
            index: u32::MAX,
            status: i32::MIN,
            flags: u32::MAX,
            signature: u64::MAX,
            payload: &payload[..max],
        });
        assert_eq!(
            Stage::out_len(max as u32),
            wire::record_framed_len(&record).unwrap()
        );
    }
}

#[test]
fn full_id_constructor_validates_locally_and_leaves_ownership_to_host() {
    let mut table = abi::host_table();
    table.stage_run = run;
    let mut fake = Fake {
        calls: Cell::new(0),
        error: abi::ERR_DENIED,
        malformed: 0,
    };
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut fake).cast(),
    };
    for id in [
        "",
        ".insert",
        "reader.",
        "reader..insert",
        "Reader.insert",
        "reader/insert",
        "reader.é",
    ] {
        assert!(matches!(Stage::new(&host, id), Err(Error::Invalid)), "{id}");
    }
    assert!(matches!(
        Stage::new(&host, &"x".repeat(129)),
        Err(Error::Invalid)
    ));
    assert_eq!(fake.calls.get(), 0);
    let long = "x".repeat(128);
    assert!(Stage::new(&host, &long).is_ok());
    let stage = Stage::new(&host, "reader.insert").unwrap();
    let mut out = [0; 200];
    assert_eq!(
        stage.run(b"input", &mut out).next().unwrap_err(),
        Error::Denied
    );
    assert_eq!(fake.calls.get(), 1);
}
