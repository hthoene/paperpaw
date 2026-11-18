//! Per-record service round trips and complete-page validation.
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
    let early = end(0, 1, 1);
    let stuck = end(0, 1, 0);
    let flags = end(2, 1, 1);
    let cursor = end(1, 1, 2);
    for records in [
        vec![&row as &dyn wire::WireEncode],
        vec![&terminal, &row],
        vec![&row, &early],
        vec![&stuck],
        vec![&row, &flags],
        vec![&row, &cursor],
    ] {
        reply(&state, &records);
        assert!(SecretPage::decode(&state.reply.borrow(), SecretList::all()).is_err());
    }
    let row = wire::ServiceSecEntry { flags: 2, ..row };
    reply(&state, &[&row, &terminal]);
    assert!(SecretPage::decode(&state.reply.borrow(), SecretList::all()).is_err());
}

#[test]
fn list_row_boundaries_truncation_and_cursor_overflow_are_checked_before_exposure() {
    let state = State::default();
    let row = entry();
    let mut rows = vec![&row as &dyn wire::WireEncode; schema::PAGE_MAX_ROWS];
    let terminal = end(1, 32, 32);
    rows.push(&terminal);
    reply(&state, &rows);
    let bytes = state.reply.borrow();
    let page = SecretPage::decode(&bytes, SecretList::all()).unwrap();
    assert_eq!(page.count(), 32);
    for len in 0..bytes.len() {
        assert!(SecretPage::decode(&bytes[..len], SecretList::all()).is_err());
    }
    drop(bytes);
    rows.insert(0, &row);
    reply(&state, &rows);
    assert!(SecretPage::decode(&state.reply.borrow(), SecretList::all()).is_err());
    reply(&state, &[&row, &end(1, 1, 0)]);
    assert!(SecretPage::decode(&state.reply.borrow(), SecretList::all().from(u32::MAX)).is_err());
    reply(&state, &[&end(1, 0, 0)]);
    assert_eq!(
        SecretPage::decode(&state.reply.borrow(), SecretList::own())
            .unwrap()
            .end()
            .total(),
        0
    );
    reply(&state, &[&end(1, 1, 99)]);
    assert!(SecretPage::decode(&state.reply.borrow(), SecretList::own().from(99)).is_ok());
}
#[test]
fn failures_and_short_buffers_do_not_call_or_publish_success() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let secrets = Secrets::open(&host).unwrap();
    let mut input = [0xa5; 1280];
    let mut out = [0xa5; 4096];
    assert!(matches!(
        secrets.get(&host, "", &mut input, &mut out),
        Err(Error::Invalid)
    ));
    assert!(matches!(
        secrets.get(&host, "token", &mut input[..1], &mut out),
        Err(Error::NoSpace)
    ));
    assert!(matches!(
        secrets.get(&host, "token", &mut input, &mut out[..1]),
        Err(Error::NoSpace)
    ));
    assert_eq!(state.calls.get(), 0);
    assert_eq!(out, [0xa5; 4096]);
    let write = SecretWrite::new(SecretName::new("token").unwrap(), "value").unwrap();
    assert_eq!(
        secrets.keep(&host, write, &mut input, &mut out[..1]),
        Err(Error::NoSpace)
    );
    assert_eq!(input, [0; 1280]);
    assert_eq!(state.calls.get(), 0);
    state.error.set(abi::ERR_IO);
    assert_eq!(
        secrets.remove(&host, write.target(), &mut input, &mut out),
        Err(Error::Io)
    );
    state.error.set(abi::OK);
    state.reply.replace(vec![0]);
    assert_eq!(
        secrets.keep(&host, write, &mut input, &mut out),
        Err(Error::Invalid)
    );
    assert_eq!(input, [0; 1280]);
    assert_eq!(
        secrets.remove(&host, write.target(), &mut input, &mut out),
        Err(Error::Invalid)
    );
    reply(
        &state,
        &[&wire::ServiceSecValue {
            flags: 2,
            label: "Label",
            value: "secret",
        }],
    );
    assert!(matches!(
        secrets.get(&host, "token", &mut input, &mut out),
        Err(Error::Invalid)
    ));
    reply(
        &state,
        &[&wire::ServiceSecBytes {
            flags: 1,
            data: &[1],
        }],
    );
    assert!(matches!(
        secrets.get(&host, "token", &mut input, &mut out),
        Err(Error::State)
    ));
    state.reply.replace(vec![0; 1281]);
    assert!(matches!(
        secrets.get(&host, "token", &mut input, &mut out),
        Err(Error::Invalid)
    ));
}

#[test]
fn failed_get_wipes_reported_reply_without_touching_unused_output() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let secrets = Secrets::open(&host).unwrap();
    let mut input = [0; 128];
    let mut out = [0xa5; 1280];
    for (label, flags) in [("bad\u{1}", 0), ("valid", 2)] {
        reply(
            &state,
            &[&wire::ServiceSecValue {
                label,
                flags,
                value: "never-leave-this-secret",
            }],
        );
        let n = state.reply.borrow().len();
        out.fill(0xa5);
        assert!(secrets.get(&host, "token", &mut input, &mut out).is_err());
        assert!(out[..n].iter().all(|b| *b == 0));
        assert!(out[n..].iter().all(|b| *b == 0xa5));
    }
    // A wrong record may still carry sensitive bytes and must be wiped as well.
    reply(
        &state,
        &[&wire::ServiceSecBytes {
            flags: 1,
            data: b"key-material",
        }],
    );
    let n = state.reply.borrow().len();
    out.fill(0xa5);
    assert!(secrets.get(&host, "token", &mut input, &mut out).is_err());
    assert!(out[..n].iter().all(|b| *b == 0));
    assert!(out[n..].iter().all(|b| *b == 0xa5));
    reply(
        &state,
        &[&wire::ServiceSecValue {
            flags: 0,
            label: "",
            value: "truncated-secret",
        }],
    );
    state.reply.borrow_mut().pop();
    let n = state.reply.borrow().len();
    out.fill(0xa5);
    assert!(secrets.get(&host, "token", &mut input, &mut out).is_err());
    assert!(out[..n].iter().all(|b| *b == 0));
}
#[test]
fn public_wipe_clears_values_random_keys_and_empty_buffers() {
    let mut secret = *b"secret";
    wipe(&mut secret);
    assert_eq!(secret, [0; 6]);
    let mut key = [0xff; 256];
    wipe(&mut key);
    assert_eq!(key, [0; 256]);
    wipe(&mut []);
}
#[test]
fn raw_random_wire_debug_redacts_bytes_and_length() {
    for data in [b"SECRET-KEY".as_slice(), &[83; 256]] {
        let raw = wire::ServiceSecBytes { flags: 1, data };
        assert_eq!(
            alloc::format!("{raw:?}"),
            "ServiceSecBytes { flags: 1, data: \"..\" }"
        );
        assert_eq!(
            alloc::format!("{:?}", wire::Record::ServiceSecBytes(raw)),
            "ServiceSecBytes(ServiceSecBytes { flags: 1, data: \"..\" })"
        );
    }
}

#[test]
fn get_wipes_whole_output_if_host_wrote_then_refused() {
    unsafe extern "C" fn write_then_fail(
        _: *mut c_void,
        _: u32,
        _: u32,
        _: PpBytes,
        out: PpBuffer,
    ) -> i32 {
        // SAFETY: the SDK passes a live exclusive output buffer of at least 1,280 bytes.
        unsafe {
            core::ptr::copy_nonoverlapping(b"secret".as_ptr(), out.ptr, 6);
        }
        abi::ERR_IO
    }
    let mut table = table();
    table.service_call = write_then_fail;
    let host = Host {
        table: &table,
        ctx: core::ptr::null_mut(),
    };
    let secrets = Secrets::open(&host).unwrap();
    let mut input = [0; 128];
    let mut out = [0xa5; 1280];
    assert!(matches!(
        secrets.get(&host, "token", &mut input, &mut out),
        Err(Error::Io)
    ));
    assert_eq!(out, [0; 1280]);
}
#[test]
fn keep_with_a_short_input_refuses_before_the_host_and_clears_its_scratch() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let secrets = Secrets::open(&host).unwrap();
    let long = "v".repeat(300);
    let write = SecretWrite::new(SecretName::new("token").unwrap(), &long).unwrap();
    // The encoder cannot hold the record: nothing reaches the host, and whatever
    // it wrote of the value in the scratch is cleared.
    for size in [0, 8, 64, 200] {
        let mut input = vec![0xa5; size];
        assert_eq!(
            secrets.keep(&host, write, &mut input, &mut [0; 64]),
            Err(Error::NoSpace),
            "input of {size} bytes"
        );
        assert!(input.iter().all(|&byte| byte == 0), "{size}: scratch");
    }
    assert_eq!(state.calls.get(), 0);
}

#[test]
fn only_the_editor_list_may_total_more_than_32_and_the_own_list_may_not() {
    let state = State::default();
    let row = entry();
    // One row of 33 in total, more to come: a valid page of the editor's list.
    reply(&state, &[&row, &end(0, 33, 1)]);
    let bytes = state.reply.borrow();
    let page = SecretPage::decode(&bytes, SecretList::all()).unwrap();
    assert_eq!((page.end().total(), page.end().next()), (33, Some(1)));
    drop(bytes);
    // A full page of 32 rows with the 33rd to come.
    let rows = vec![&row as &dyn wire::WireEncode; schema::PAGE_MAX_ROWS];
    let terminal = end(0, 33, 32);
    let mut full = rows.clone();
    full.push(&terminal);
    reply(&state, &full);
    let bytes = state.reply.borrow();
    let page = SecretPage::decode(&bytes, SecretList::all()).unwrap();
    assert_eq!((page.end().total(), page.clone().count()), (33, 32));
    drop(bytes);
    // The same pages are refused for the plugin's own list: it holds at most 32.
    reply(&state, &[&row, &end(0, 33, 1)]);
    assert!(SecretPage::decode(&state.reply.borrow(), SecretList::own()).is_err());
    reply(&state, &full);
    assert!(SecretPage::decode(&state.reply.borrow(), SecretList::own()).is_err());
    // 32 in total is the own list's most.
    let terminal = end(1, 32, 32);
    let mut own = rows;
    own.push(&terminal);
    reply(&state, &own);
    assert!(SecretPage::decode(&state.reply.borrow(), SecretList::own()).is_ok());
}

#[test]
fn a_secret_that_is_not_changeable_has_no_replacement() {
    let state = State::default();
    let fixed = wire::ServiceSecEntry {
        flags: 0,
        ..entry()
    };
    reply(&state, &[&fixed, &end(1, 1, 1)]);
    let bytes = state.reply.borrow();
    let mut page = SecretPage::decode(&bytes, SecretList::all()).unwrap();
    let row = page.next().unwrap().unwrap();
    assert!(!row.changeable());
    assert!(matches!(row.replacement("new"), Err(Error::Denied)));
    // Denied wins over a value that is too long as well: the owner decides first.
    assert!(matches!(
        row.replacement(&"v".repeat(2000)),
        Err(Error::Denied)
    ));
    drop(bytes);
    // A changeable one takes the value.
    let open = entry();
    reply(&state, &[&open, &end(1, 1, 1)]);
    let bytes = state.reply.borrow();
    let mut page = SecretPage::decode(&bytes, SecretList::all()).unwrap();
    assert!(page.next().unwrap().unwrap().replacement("new").is_ok());
}
