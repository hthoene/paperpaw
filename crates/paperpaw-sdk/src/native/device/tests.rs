#![allow(clippy::unwrap_used, reason = "fixture refusals must fail tests")]
use super::*;
use crate::native::declare::records_wire::Record;
use alloc::{vec, vec::Vec};
use core::{
    cell::{Cell, RefCell},
    ffi::c_void,
};
use paperpaw_native::host_glue::{self as abi, PpBuffer, PpBytes, PpHost};
#[derive(Default)]
struct State {
    op: Cell<u32>,
    calls: Cell<u32>,
    input: RefCell<Vec<u8>>,
    reply: RefCell<Vec<u8>>,
    error: Cell<i32>,
}
unsafe extern "C" fn open(
    _: *mut c_void,
    key: PpBytes,
    major: u16,
    minor: u16,
    out: *mut u32,
) -> i32 {
    // SAFETY: SDK provides a live key and aligned output.
    unsafe {
        assert_eq!(
            core::slice::from_raw_parts(key.ptr, key.len),
            b"core.device"
        );
        out.write(7);
    }
    assert_eq!((major, minor), (1, 0));
    abi::OK
}
unsafe extern "C" fn call(ctx: *mut c_void, _: u32, op: u32, input: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: fixture state and SDK buffers remain live through this call.
    let state = unsafe { &*ctx.cast::<State>() };
    state.op.set(op);
    state.calls.set(state.calls.get() + 1);
    // SAFETY: SDK encoded a live input slice.
    state
        .input
        .replace(unsafe { core::slice::from_raw_parts(input.ptr, input.len) }.to_vec());
    if state.error.get() < 0 {
        return state.error.get();
    }
    let reply = state.reply.borrow();
    assert!(reply.len() <= out.cap);
    // SAFETY: the fixture reply and exclusive SDK output are disjoint.
    unsafe {
        core::ptr::copy_nonoverlapping(reply.as_ptr(), out.ptr, reply.len());
    }
    i32::try_from(reply.len()).unwrap()
}
fn table() -> PpHost {
    let mut t = abi::host_table();
    t.service_open = open;
    t.service_call = call;
    t
}
fn reply(state: &State, rows: &[Record<'_>]) {
    let mut bytes = vec![0; 4096];
    let mut n = 0;
    for row in rows {
        n += wire::record_encode_one(row, &mut bytes[n..]).unwrap();
    }
    bytes.truncate(n);
    state.reply.replace(bytes);
}
fn identity() -> Record<'static> {
    Record::ServiceDevDevice(wire::ServiceDevDevice {
        id: "x4",
        model: "X4",
        catalog: "x4",
        class: 0,
        caps: 5,
        controls: 3,
        tls_sessions: 2,
        start: 2,
        slept_ms: 123,
        flags: 1,
    })
}
fn builtin() -> Record<'static> {
    Record::ServiceDevBuiltin(wire::ServiceDevBuiltin {
        id: "demo",
        version: "1.0",
    })
}
fn end(flags: u32, total: u32, generation: u32, next: u32) -> Record<'static> {
    Record::ServiceEnd(wire::ServiceEnd {
        flags,
        total,
        generation,
        next,
        cursor: None,
    })
}

#[test]
fn about_inventory_reads_use_schema_operations_and_borrowed_values() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let device = Device::open(&host).unwrap();
    let mut input = [0; 256];
    let mut out = [0; 4096];
    reply(&state, &[identity()]);
    let row = device.device(&host, &mut input, &mut out).unwrap();
    assert_eq!(
        (row.id(), row.model(), row.class()),
        ("x4", "X4", DeviceClass::Small)
    );
    assert_eq!(state.op.get(), schema::OP_DEVICE);
    assert!(
        wire::records_decode(&state.input.borrow())
            .unwrap()
            .is_empty()
    );
    reply(
        &state,
        &[Record::ServiceDevFirmware(wire::ServiceDevFirmware {
            version: "1.0",
            project: "PaperPaw",
            chip: 1,
            room: 123,
            undone: 0,
            state: 0,
            done: 0,
            total: 0,
            reason: 99,
            request: 0,
            flags: 0,
        })],
    );
    let row = device.firmware(&host, &mut input, &mut out).unwrap();
    assert_eq!(
        (row.version(), row.project(), row.reason()),
        ("1.0", "PaperPaw", DeviceOutcome::Other)
    );
    reply(
        &state,
        &[Record::ServiceDevNative(wire::ServiceDevNative {
            abi_major: 7,
            abi_draft: 20,
            target: 1,
            code_file: "code.bin",
            safe_mode: 0,
            crashed: "",
            last_loaded: "demo",
            pending: 0,
            not_installed: 0,
            room_free: 10,
            room_largest: 8,
        })],
    );
    let row = device.native(&host, &mut input, &mut out).unwrap();
    assert_eq!(
        (row.abi_major(), row.abi_draft(), row.code_file()),
        (7, 20, "code.bin")
    );
    reply(
        &state,
        &[Record::ServiceDevMemory(wire::ServiceDevMemory {
            heap_free: 10,
            heap_size: 20,
            largest: 5,
            stack_free: 4,
            stack_size: 8,
            external_free: 100,
            external_size: 200,
            flags: 0,
        })],
    );
    let row = device.memory(&host, &mut input, &mut out).unwrap();
    assert_eq!(
        (
            row.heap_free(),
            row.largest_block(),
            row.stack_free(),
            row.external_free()
        ),
        (10, None, None, None)
    );
}
#[test]
fn builtins_continue_only_the_same_generation_and_validate_every_row() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let device = Device::open(&host).unwrap();
    let mut input = [0; 256];
    let mut out = [0; 4096];
    reply(&state, &[builtin(), end(0, 2, 4, 1)]);
    let page = device
        .builtins(&host, DeviceCursor::start(), &mut input, &mut out)
        .unwrap();
    assert_eq!(page.rows().next().unwrap().id(), "demo");
    let next = page.end().next_cursor().unwrap();
    reply(&state, &[builtin(), end(1, 2, 5, 2)]);
    assert!(matches!(
        device.builtins(&host, next, &mut input, &mut out),
        Err(Error::State)
    ));
    reply(&state, &[builtin(), end(1, 2, 4, 2)]);
    let page = device.builtins(&host, next, &mut input, &mut out).unwrap();
    assert!(page.end().next_cursor().is_none());
    let bytes = state.input.borrow();
    let batch = wire::records_decode(&bytes).unwrap();
    assert!(
        matches!(batch.iter().next(),Some(Record::ServiceRegQuery(r)) if (r.cursor,r.generation)==(1,4))
    );
    drop(bytes);
    for ending in [
        end(0, 1, 4, 1),
        end(1, 1, 0, 1),
        end(1, 1, 4, 2),
        end(3, 1, 4, 1),
    ] {
        reply(&state, &[builtin(), ending]);
        assert!(
            device
                .builtins(&host, DeviceCursor::start(), &mut input, &mut out)
                .is_err()
        );
    }
    reply(&state, &[builtin()]);
    assert!(
        device
            .builtins(&host, DeviceCursor::start(), &mut input, &mut out)
            .is_err()
    );
    reply(&state, &[builtin(), end(1, 1, 4, 1), builtin()]);
    assert!(
        device
            .builtins(&host, DeviceCursor::start(), &mut input, &mut out)
            .is_err()
    );
    let mut rows = vec![builtin(); schema::PAGE_MAX_ROWS + 1];
    rows.push(end(1, 33, 4, 33));
    reply(&state, &rows);
    assert!(
        device
            .builtins(&host, DeviceCursor::start(), &mut input, &mut out)
            .is_err()
    );
}
#[test]
fn hardware_values_require_unpaged_end_caps_and_valid_names() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let device = Device::open(&host).unwrap();
    let mut input = [0; 256];
    let mut out = [0; 4096];
    let row = Record::ServiceDevValue(wire::ServiceDevValue {
        name: "battery-percent",
        value: 85,
    });
    reply(&state, &[row, end(0, 1, 0, 0)]);
    assert_eq!(
        device
            .values(&host, &mut input, &mut out)
            .unwrap()
            .rows()
            .next()
            .unwrap()
            .value(),
        85
    );
    for ending in [
        end(1, 1, 0, 0),
        end(0, 1, 1, 0),
        end(0, 1, 0, 1),
        end(0, 2, 0, 0),
    ] {
        reply(&state, &[row, ending]);
        assert!(device.values(&host, &mut input, &mut out).is_err());
    }
    reply(
        &state,
        &[
            Record::ServiceDevValue(wire::ServiceDevValue {
                name: "Bad Name",
                value: 0,
            }),
            end(0, 1, 0, 0),
        ],
    );
    assert!(device.values(&host, &mut input, &mut out).is_err());
    let mut rows = vec![row; 9];
    rows.push(end(0, 9, 0, 0));
    reply(&state, &rows);
    assert!(device.values(&host, &mut input, &mut out).is_err());
}
#[test]
fn wrong_scalar_tags_trailing_bytes_and_host_refusals_are_not_hidden() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let device = Device::open(&host).unwrap();
    let mut input = [0; 256];
    let mut out = [0; 4096];
    reply(&state, &[builtin()]);
    assert!(matches!(
        device.device(&host, &mut input, &mut out),
        Err(Error::Invalid)
    ));
    reply(&state, &[identity(), identity()]);
    assert!(device.device(&host, &mut input, &mut out).is_err());
    state.error.set(abi::ERR_BUSY);
    assert!(matches!(
        device.device(&host, &mut input, &mut out),
        Err(Error::Busy)
    ));
    assert!(DeviceRequest::new(0).is_none());
}

fn bounded_rows(op: u32, count: usize) -> Vec<Record<'static>> {
    let row = match op {
        schema::OP_BONDED => Record::ServiceDevBonded(wire::ServiceDevBonded {
            device: 1,
            name: "Reader",
            kind: 0,
            flags: 0,
            given: "",
        }),
        schema::OP_NETWORKS => Record::ServiceDevNetwork(wire::ServiceDevNetwork {
            ssid: "Network",
            signal: -42,
            flags: 0,
        }),
        _ => Record::ServiceDevNotInstalled(wire::ServiceDevNotInstalled {
            plugin: "demo",
            reason: 1,
        }),
    };
    vec![row; count]
}
fn bounded_list(
    device: &Device,
    host: &Host<'_>,
    op: u32,
    input: &mut [u8],
    out: &mut [u8],
) -> Result<usize, Error> {
    match op {
        schema::OP_BONDED => device
            .bonded(host, DeviceCursor::start(), input, out)
            .map(|p| p.len()),
        schema::OP_NETWORKS => device
            .networks(host, DeviceCursor::start(), input, out)
            .map(|p| p.len()),
        _ => device
            .not_installed(host, DeviceCursor::start(), input, out)
            .map(|p| p.len()),
    }
}
#[test]
fn bounded_device_tables_reject_oversized_rows_and_totals() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let device = Device::open(&host).unwrap();
    let mut input = [0; 256];
    let mut out = [0; 4096];
    for (op, limit) in [
        (schema::OP_BONDED, 16_u16),
        (schema::OP_NETWORKS, 32),
        (schema::OP_NOT_INSTALLED, 32),
    ] {
        let mut rows = bounded_rows(op, usize::from(limit));
        rows.push(end(1, u32::from(limit), 7, u32::from(limit)));
        reply(&state, &rows);
        assert_eq!(
            bounded_list(&device, &host, op, &mut input, &mut out),
            Ok(usize::from(limit))
        );
        let mut rows = bounded_rows(op, 1);
        rows.push(end(0, u32::from(limit), 7, 1));
        reply(&state, &rows);
        assert_eq!(
            bounded_list(&device, &host, op, &mut input, &mut out),
            Ok(1)
        );
        rows.pop();
        rows.push(end(0, u32::from(limit) + 1, 7, 1));
        reply(&state, &rows);
        assert!(matches!(
            bounded_list(&device, &host, op, &mut input, &mut out),
            Err(Error::Invalid)
        ));
        let mut rows = bounded_rows(op, usize::from(limit) + 1);
        rows.push(end(1, u32::from(limit) + 1, 7, u32::from(limit) + 1));
        reply(&state, &rows);
        assert!(matches!(
            bounded_list(&device, &host, op, &mut input, &mut out),
            Err(Error::Invalid)
        ));
    }
}

#[test]
fn power_control_bits_are_independent_and_unknown_bits_are_ignored() {
    let controls = [
        PowerControl::Radio,
        PowerControl::CpuClock,
        PowerControl::Buttons,
        PowerControl::Frontlight,
        PowerControl::SystemSaver,
    ];
    for (bit, expected) in controls.iter().enumerate() {
        let Record::ServiceDevDevice(mut row) = identity() else {
            panic!("fixture")
        };
        row.controls = (1 << bit) | (1 << 31);
        let mut bytes = [0_u8; 512];
        let n = wire::record_encode_of(&row, &mut bytes).unwrap();
        let device = DeviceIdentity::decode(RawRecord::one(&bytes[..n]).unwrap()).unwrap();
        for control in controls {
            assert_eq!(device.can_control(control), control == *expected);
        }
    }
}
#[test]
fn radio_power_and_request_calls_use_exact_records_and_checked_identity() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let device = Device::open(&host).unwrap();
    let mut input = [0; 256];
    let mut out = [0; 4096];
    reply(
        &state,
        &[Record::ServiceDevWifi(wire::ServiceDevWifi {
            state: 99,
            ssid: "wifi",
            address: "",
            host_name: "reader",
            guests: 2,
            flags: 5,
        })],
    );
    let wifi = device.wifi(&host, &mut input, &mut out).unwrap();
    assert_eq!(wifi.state(), WifiState::Unknown(99));
    assert!(wifi.scanning() && wifi.busy() && !wifi.wanted());
    assert_eq!(state.op.get(), schema::OP_WIFI);
    reply(
        &state,
        &[Record::ServiceDevBluetooth(wire::ServiceDevBluetooth {
            radio: 1,
            max_links: 3,
            flags: 0,
        })],
    );
    assert_eq!(
        device
            .bluetooth(&host, &mut input, &mut out)
            .unwrap()
            .max_links(),
        3
    );
    assert_eq!(state.op.get(), schema::OP_BLUETOOTH);
    reply(
        &state,
        &[Record::ServiceDevPower(wire::ServiceDevPower {
            level: 1,
            mode: "core.saving",
            flags: 3,
            stretch: 2,
            every_ms: 600,
        })],
    );
    let power = device.power(&host, 300, &mut input, &mut out).unwrap();
    assert_eq!((power.level(), power.every_ms()), (PowerLevel::Saving, 600));
    assert!(power.automatic() && power.online());
    let bytes = state.input.borrow();
    let batch = wire::records_decode(&bytes).unwrap();
    assert!(matches!(batch.iter().next(),Some(Record::ServiceDevInterval(r)) if r.base_ms==300));
    drop(bytes);
    let request = DeviceRequest::new(42).unwrap();
    let install = wire::ServiceDevInstall {
        request: 42,
        kind: 0,
        state: 0,
        status: 0,
        reason: 99,
        flags: 3,
        done: 0,
        total: 1,
        id: "demo",
    };
    reply(&state, &[Record::ServiceDevInstall(install)]);
    let row = device
        .install(&host, request, &mut input, &mut out)
        .unwrap();
    assert_eq!(row.reason(), DeviceOutcome::Other);
    assert!(row.data_kept() && row.restart_required());
    let bytes = state.input.borrow();
    let batch = wire::records_decode(&bytes).unwrap();
    assert!(matches!(batch.iter().next(),Some(Record::ServiceDevRequest(r)) if r.request==42));
    drop(bytes);
    reply(
        &state,
        &[Record::ServiceDevInstall(wire::ServiceDevInstall {
            request: 43,
            ..install
        })],
    );
    assert!(matches!(
        device.install(&host, request, &mut input, &mut out),
        Err(Error::State)
    ));
}
#[test]
fn check_and_keys_validate_unsigned_evidence_empty_lists_and_truncation() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let device = Device::open(&host).unwrap();
    let mut input = [0; 256];
    let mut out = [0; 4096];
    reply(
        &state,
        &[Record::ServiceDevCheck(wire::ServiceDevCheck {
            reason: 8,
            needs_draft: 0,
            bytes: 123,
            signer: "",
            key: &[],
            sha256: &[],
            id: "demo",
            version: "1",
            flags: 0,
        })],
    );
    let check = device
        .check(&host, DeviceRequest::new(9).unwrap(), &mut input, &mut out)
        .unwrap();
    assert_eq!(check.reason(), DeviceOutcome::Unsigned);
    assert!(check.signing_key().is_none());
    assert!(check.sha256().is_empty());
    assert_eq!(state.op.get(), schema::OP_CHECK);
    let bytes = state.input.borrow();
    let batch = wire::records_decode(&bytes).unwrap();
    assert!(matches!(batch.iter().next(),Some(Record::ServiceDevRequest(r)) if r.request==9));
    drop(bytes);
    reply(&state, &[end(0, 0, 0, 0)]);
    assert!(device.keys(&host, &mut input, &mut out).unwrap().is_empty());
    let key = Record::ServiceDevKey(wire::ServiceDevKey {
        name: "button.menu",
    });
    let mut rows = vec![key; 32];
    rows.push(end(0, 32, 0, 0));
    reply(&state, &rows);
    assert_eq!(device.keys(&host, &mut input, &mut out).unwrap().len(), 32);
    rows.insert(0, key);
    reply(&state, &rows);
    assert!(device.keys(&host, &mut input, &mut out).is_err());
    reply(&state, &[identity()]);
    let complete = state.reply.borrow().clone();
    for n in 0..complete.len() {
        state.reply.replace(complete[..n].to_vec());
        assert!(device.device(&host, &mut input, &mut out).is_err());
    }
}

#[test]
fn file_check_digest_is_borrowed_exactly_or_absent() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let device = Device::open(&host).unwrap();
    let mut input = [0; 256];
    let mut out = [0; 4096];
    let digest = [0x42; 32];
    for sha256 in [&[][..], &digest[..]] {
        reply(
            &state,
            &[Record::ServiceDevCheck(wire::ServiceDevCheck {
                reason: 0,
                needs_draft: 0,
                bytes: 123,
                signer: "Author",
                key: &digest,
                id: "demo",
                version: "1",
                flags: 0,
                sha256,
            })],
        );
        let start = out.as_ptr().addr();
        let end = start + out.len();
        let row = device
            .check(&host, DeviceRequest::new(1).unwrap(), &mut input, &mut out)
            .unwrap();
        assert_eq!(row.sha256(), sha256);
        if !sha256.is_empty() {
            assert!((start..end).contains(&row.sha256().as_ptr().addr()));
        }
    }
    reply(
        &state,
        &[Record::ServiceDevCheck(wire::ServiceDevCheck {
            reason: 0,
            needs_draft: 0,
            bytes: 123,
            signer: "Author",
            key: &digest,
            id: "demo",
            version: "1",
            flags: 0,
            sha256: &digest[..31],
        })],
    );
    assert!(matches!(
        device.check(&host, DeviceRequest::new(1).unwrap(), &mut input, &mut out),
        Err(Error::Invalid)
    ));
}

#[test]
fn leaves_out_pages_under_the_checks_request_and_the_hosts_refusals_are_the_status() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let device = Device::open(&host).unwrap();
    let mut input = [0; 256];
    let mut out = [0; 4096];
    // The first page: generation 0 sent; the end carries the check's request.
    reply(&state, &[builtin(), end(0, 2, 9, 1)]);
    let page = device
        .leaves_out(&host, DeviceCursor::start(), &mut input, &mut out)
        .unwrap();
    assert_eq!(state.op.get(), schema::OP_LEAVES_OUT);
    assert_eq!(page.rows().next().unwrap().id(), "demo");
    let next = page.end().next_cursor().unwrap();
    // A cursor goes back with the end's generation; another one is stale.
    reply(&state, &[builtin(), end(1, 2, 8, 2)]);
    assert!(matches!(
        device.leaves_out(&host, next, &mut input, &mut out),
        Err(Error::State)
    ));
    reply(&state, &[builtin(), end(1, 2, 9, 2)]);
    assert!(
        device
            .leaves_out(&host, next, &mut input, &mut out)
            .unwrap()
            .end()
            .next_cursor()
            .is_none()
    );
    let bytes = state.input.borrow();
    let sent = wire::records_decode(&bytes).unwrap();
    assert!(matches!(
        sent.iter().next(),
        Some(Record::ServiceRegQuery(q)) if (q.scope, q.cursor, q.generation) == ("", 1, 9)
    ));
    drop(bytes);
    // None left out: no rows.
    reply(&state, &[end(1, 0, 9, 0)]);
    assert!(
        device
            .leaves_out(&host, DeviceCursor::start(), &mut input, &mut out)
            .unwrap()
            .is_empty()
    );
    // No check, one running, a foreign image: the call's status.
    for (status, error) in [
        (abi::ERR_NOT_FOUND, Error::NotFound),
        (abi::ERR_BUSY, Error::Busy),
        (abi::ERR_UNSUPPORTED, Error::Unsupported),
    ] {
        state.error.set(status);
        assert_eq!(
            device
                .leaves_out(&host, DeviceCursor::start(), &mut input, &mut out)
                .map(|page| page.len()),
            Err(error)
        );
    }
    state.error.set(0);
    // More rows than a page, a row after the end or a missing end: Invalid.
    let mut rows = vec![builtin(); schema::PAGE_MAX_ROWS + 1];
    rows.push(end(1, 65, 9, 65));
    reply(&state, &rows);
    assert!(
        device
            .leaves_out(&host, DeviceCursor::start(), &mut input, &mut out)
            .is_err()
    );
    reply(&state, &[builtin()]);
    assert!(
        device
            .leaves_out(&host, DeviceCursor::start(), &mut input, &mut out)
            .is_err()
    );
}

#[test]
fn file_check_leaves_out_reads_flag_bit_5_alone() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let device = Device::open(&host).unwrap();
    let mut input = [0; 256];
    let mut out = [0; 4096];
    for (flags, leaves_out) in [(0, false), (1 << 5, true), (0b01_1111, false), (!0, true)] {
        reply(
            &state,
            &[Record::ServiceDevCheck(wire::ServiceDevCheck {
                reason: 0,
                needs_draft: 0,
                bytes: 1,
                signer: "",
                key: &[],
                id: "firmware",
                version: "1",
                flags,
                sha256: &[],
            })],
        );
        let row = device
            .check(&host, DeviceRequest::new(1).unwrap(), &mut input, &mut out)
            .unwrap();
        assert_eq!(row.leaves_out(), leaves_out, "flags {flags:#x}");
    }
}
