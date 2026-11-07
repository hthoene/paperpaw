//! Enabled with the typed codec contract and the service-call modules.
use super::paging::SecretPage;
use super::*;
use alloc::{vec, vec::Vec};
use core::{
    cell::{Cell, RefCell},
    ffi::c_void,
};
use paperpaw_native::host_glue::{self as abi, PpBuffer, PpBytes, PpHost};

#[derive(Default)]
struct State {
    input: RefCell<Vec<u8>>,
    reply: RefCell<Vec<u8>>,
    op: Cell<u32>,
    calls: Cell<u32>,
    error: Cell<i32>,
    released: Cell<bool>,
}
unsafe extern "C" fn open(
    _: *mut c_void,
    key: PpBytes,
    major: u16,
    minor: u16,
    out: *mut u32,
) -> i32 {
    // SAFETY: test slots receive live SDK inputs and exclusive output storage.
    unsafe {
        assert_eq!(
            core::slice::from_raw_parts(key.ptr, key.len),
            b"core.secrets"
        );
        out.write(7);
    }
    assert_eq!((major, minor), (1, 0));
    abi::OK
}
unsafe extern "C" fn call(
    ctx: *mut c_void,
    handle: u32,
    op: u32,
    input: PpBytes,
    out: PpBuffer,
) -> i32 {
    assert_eq!(handle, 7);
    // SAFETY: the fixture state and SDK buffers are valid throughout this call.
    let state = unsafe { &*ctx.cast::<State>() };
    state.calls.set(state.calls.get() + 1);
    state.op.set(op);
    // SAFETY: the SDK supplies a readable counted input batch.
    state
        .input
        .replace(unsafe { core::slice::from_raw_parts(input.ptr, input.len) }.to_vec());
    if state.error.get() < 0 {
        return state.error.get();
    }
    let reply = state.reply.borrow();
    assert!(reply.len() <= out.cap);
    // SAFETY: reply storage and exclusive output storage are disjoint.
    unsafe {
        core::ptr::copy_nonoverlapping(reply.as_ptr(), out.ptr, reply.len());
    }
    i32::try_from(reply.len()).unwrap()
}
unsafe extern "C" fn release(ctx: *mut c_void, handle: u32) -> i32 {
    assert_eq!(handle, 7);
    // SAFETY: the context remains alive until the handle is released.
    let state = unsafe { &*ctx.cast::<State>() };
    assert!(!state.released.replace(true));
    state.error.get()
}
fn table() -> PpHost {
    let mut table = abi::host_table();
    table.service_open = open;
    table.service_call = call;
    table.handle_release = release;
    table
}
fn reply(state: &State, rows: &[&dyn wire::WireEncode]) {
    let mut bytes = vec![0; 4096];
    let mut used = 0;
    for row in rows {
        used += wire::record_encode_of(*row, &mut bytes[used..]).unwrap();
    }
    bytes.truncate(used);
    state.reply.replace(bytes);
}
fn end(flags: u32, total: u32, next: u32) -> wire::ServiceEnd {
    wire::ServiceEnd {
        flags,
        total,
        generation: 0,
        next,
        cursor: None,
    }
}
fn entry() -> wire::ServiceSecEntry<'static> {
    wire::ServiceSecEntry {
        flags: 1,
        plugin: "wifi",
        name: "a\t\n\\SSID",
        label: "Network",
    }
}
#[test]
fn own_reads_and_editor_writes_use_typed_records_and_wipe_keep_input() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let secrets = Secrets::open(&host).unwrap();
    let mut input = [0xa5; 1280];
    let mut out = [0; 1280];
    reply(
        &state,
        &[&wire::ServiceSecValue {
            flags: 1,
            label: "Label",
            value: "secret-token",
        }],
    );
    let range = out.as_ptr_range();
    let value = secrets.get(&host, "token", &mut input, &mut out).unwrap();
    assert_eq!(
        (value.value(), value.label(), value.changeable()),
        ("secret-token", "Label", true)
    );
    assert!(range.contains(&value.value().as_ptr()));
    let raw_input = state.input.borrow();
    let request = wire::RawRecord::single(&raw_input)
        .unwrap()
        .decode::<wire::ServiceSecName<'_>>()
        .unwrap();
    assert_eq!((request.plugin, request.name), ("", "token"));
    drop(raw_input);
    state.reply.borrow_mut().clear();
    let target = SecretName::new("a\t\n\\SSID")
        .unwrap()
        .owner("wifi")
        .unwrap();
    let write = SecretWrite::new(target, "new-password")
        .unwrap()
        .label("Network")
        .unwrap()
        .changeable();
    secrets.keep(&host, write, &mut input, &mut out).unwrap();
    assert_eq!(input, [0; 1280]);
    let bytes = state.input.borrow();
    let request = wire::RawRecord::single(&bytes)
        .unwrap()
        .decode::<wire::ServiceSecKeep<'_>>()
        .unwrap();
    assert_eq!(
        (
            request.plugin,
            request.name,
            request.label,
            request.value,
            request.flags
        ),
        ("wifi", "a\t\n\\SSID", "Network", "new-password", 1)
    );
    drop(bytes);
    state.error.set(abi::ERR_DENIED);
    input.fill(0xa5);
    assert_eq!(
        secrets.keep(&host, write, &mut input, &mut out),
        Err(Error::Denied)
    );
    assert_eq!(input, [0; 1280]);
    state.error.set(abi::OK);
    secrets.remove(&host, target, &mut input, &mut out).unwrap();
    assert_eq!(state.op.get(), schema::OP_REMOVE);
    let bytes = state.input.borrow();
    let request = wire::RawRecord::single(&bytes)
        .unwrap()
        .decode::<wire::ServiceSecName<'_>>()
        .unwrap();
    assert_eq!((request.plugin, request.name), ("wifi", "a\t\n\\SSID"));
    drop(bytes);
    secrets.release(&host).unwrap();
    assert!(state.released.get());
}
#[test]
fn random_checks_exact_length_flags_limits_and_source_errors() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let secrets = Secrets::open(&host).unwrap();
    let mut input = [0; 256];
    let mut out = [0; 1280];
    for len in [0, 257, usize::MAX] {
        assert!(matches!(
            secrets.random(&host, len, &mut input, &mut out),
            Err(Error::Invalid)
        ));
    }
    assert_eq!(state.calls.get(), 0);
    for flags in [0, 1] {
        reply(
            &state,
            &[&wire::ServiceSecBytes {
                flags,
                data: &[0x5a; 256],
            }],
        );
        let draw = secrets.random(&host, 256, &mut input, &mut out).unwrap();
        assert_eq!(draw.bytes(), &[0x5a; 256]);
        assert_eq!(draw.strong(), flags == 1);
    }
    for (flags, data) in [(2, &b"12"[..]), (0, &b"1"[..]), (0, &b"123"[..])] {
        reply(&state, &[&wire::ServiceSecBytes { flags, data }]);
        assert!(matches!(
            secrets.random(&host, 2, &mut input, &mut out),
            Err(Error::Invalid)
        ));
    }
    state.error.set(abi::ERR_UNSUPPORTED);
    assert!(matches!(
        secrets.random(&host, 2, &mut input, &mut out),
        Err(Error::Unsupported)
    ));
    state.error.set(abi::OK);
    secrets.release(&host).unwrap();
}
#[test]
fn secret_pages_are_borrowed_complete_and_preserve_editor_flags() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let secrets = Secrets::open(&host).unwrap();
    let mut input = [0; 256];
    let mut out = [0; 4096];
    reply(&state, &[&entry(), &end(0, 2, 1)]);
    let range = out.as_ptr_range();
    let mut page = secrets
        .list(&host, SecretList::all(), &mut input, &mut out)
        .unwrap();
    assert_eq!((page.end().total(), page.end().next()), (2, Some(1)));
    let row = page.next().unwrap().unwrap();
    assert!(range.contains(&row.name().as_ptr()));
    let replacement = row.replacement("new").unwrap();
    assert_eq!(replacement.target().plugin(), "wifi");
    assert!(replacement.is_changeable());
    assert!(page.next().is_none());
    reply(&state, &[&entry(), &end(1, 2, 2)]);
    let page = secrets
        .list(&host, SecretList::all().from(1), &mut input, &mut out)
        .unwrap();
    assert_eq!((page.end().total(), page.end().next()), (2, None));
    assert_eq!(page.count(), 1);
    state.error.set(abi::ERR_DENIED);
    assert!(matches!(
        secrets.list(&host, SecretList::all(), &mut input, &mut out),
        Err(Error::Denied)
    ));
}
#[test]
fn list_refuses_missing_terminal_trailing_rows_bad_flags_and_nonprogress() {
    let state = State::default();
    let row = entry();
    let terminal = end(1, 1, 1);
    for records in [
        vec![&row as &dyn wire::WireEncode],
        vec![&terminal, &row],
        vec![&row, &end(0, 1, 1)],
        vec![&end(0, 1, 0)],
        vec![&row, &end(2, 1, 1)],
        vec![&row, &end(1, 1, 2)],
    ] {
        reply(&state, &records);
        assert!(SecretPage::decode(&state.reply.borrow(), SecretList::all()).is_err());
    }
    let row = wire::ServiceSecEntry { flags: 2, ..row };
    reply(&state, &[&row, &terminal]);
    assert!(SecretPage::decode(&state.reply.borrow(), SecretList::all()).is_err());
}
