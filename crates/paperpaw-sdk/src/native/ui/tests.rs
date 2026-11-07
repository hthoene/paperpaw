//! Client refusal paths and complete UTF-8 texts use the generated codec.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use alloc::{vec, vec::Vec};
use core::{
    cell::{Cell, RefCell},
    ffi::c_void,
};
use paperpaw_native::host_glue::{self as abi, PpBuffer, PpBytes, PpHost};

#[derive(Default)]
struct State {
    calls: Cell<u32>,
    input: RefCell<Vec<u8>>,
    reply: RefCell<Vec<u8>>,
    result: Cell<i32>,
    composed: Cell<Option<(u32, u32, paperpaw_platform::Rect)>>,
    compose_result: Cell<i32>,
}
unsafe extern "C" fn open(_: *mut c_void, _: PpBytes, _: u16, _: u16, out: *mut u32) -> i32 {
    // SAFETY: the client supplies aligned exclusive output storage.
    unsafe {
        out.write(9);
    }
    abi::OK
}
unsafe extern "C" fn call(ctx: *mut c_void, _: u32, _: u32, input: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: this fixture's host binding keeps State and call buffers alive.
    let state = unsafe { &*ctx.cast::<State>() };
    state.calls.set(state.calls.get() + 1);
    // SAFETY: input comes from the generated encoder's initialized scratch.
    state
        .input
        .replace(unsafe { core::slice::from_raw_parts(input.ptr, input.len) }.to_vec());
    if state.result.get() < 0 {
        return state.result.get();
    }
    let reply = state.reply.borrow();
    assert!(reply.len() <= out.cap);
    // SAFETY: disjoint host-owned reply and client-exclusive output buffers.
    unsafe {
        core::ptr::copy_nonoverlapping(reply.as_ptr(), out.ptr, reply.len());
    }
    i32::try_from(reply.len()).unwrap()
}
fn table() -> PpHost {
    let mut table = abi::host_table();
    table.service_open = open;
    table.ui_compose = compose;
    table.commands_submit = submit;
    table.service_call = call;
    table
}
fn set_reply(state: &State, record: wire::Record<'_>) {
    let mut bytes = vec![0; 4096];
    let len = wire::record_encode_one(&record, &mut bytes).unwrap();
    bytes.truncate(len);
    state.reply.replace(bytes);
}

#[test]
fn ask_encodes_text_flags_and_validates_request_reply() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let ui = Ui::open(&host).unwrap();
    let mut input = [0; 4096];
    let mut out = [0; 4096];
    set_reply(
        &state,
        wire::Record::ServiceUiRequest(wire::ServiceUiRequest { request: 41 }),
    );
    let request = ui
        .ask(
            &host,
            Question::text("Title", "Question")
                .unwrap()
                .live()
                .unwrap()
                .secret()
                .unwrap()
                .multiline()
                .unwrap()
                .mode(TextMode::Email)
                .unwrap()
                .remote()
                .unwrap(),
            &mut input,
            &mut out,
        )
        .unwrap();
    assert_eq!(request.get(), 41);
    assert_eq!(request.text().get(), 41);
    let bytes = state.input.borrow();
    let decoded = wire::records_decode(&bytes).unwrap();
    let Some(wire::Record::ServiceUiAsk(row)) = decoded.iter().next() else {
        panic!("expected ask");
    };
    assert_eq!(
        (row.kind, row.flags, row.slot),
        (0, 0x27, "core.remote-keyboard")
    );
    drop(bytes);
    set_reply(
        &state,
        wire::Record::ServiceUiRequest(wire::ServiceUiRequest { request: 0 }),
    );
    assert!(matches!(
        ui.ask(
            &host,
            Question::confirm("", "").unwrap(),
            &mut input,
            &mut out
        ),
        Err(Error::Invalid)
    ));
    state.result.set(abi::ERR_DENIED);
    assert!(matches!(
        ui.ask(
            &host,
            Question::confirm("", "").unwrap(),
            &mut input,
            &mut out
        ),
        Err(Error::Denied)
    ));
}

#[test]
fn question_caps_and_kind_flags_refuse_before_host_calls() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let ui = Ui::open(&host).unwrap();
    let mut input = [0; 4096];
    let mut out = [0; 4096];
    for question in [
        Question::confirm("", "").unwrap().live(),
        Question::folder("", "").unwrap().remote(),
        Question::choice("", "", ""),
        Question::choice("", "", "a\n"),
        Question::choice("", "", "a\n\nb"),
    ] {
        assert!(matches!(question, Err(Error::Invalid)));
    }
    let long_choice = "x".repeat(129);
    assert!(matches!(
        Question::choice("", "", &long_choice),
        Err(Error::NoSpace)
    ));
    assert_eq!(state.calls.get(), 0);
    let title = "t".repeat(256);
    let text = "é".repeat(512);
    set_reply(
        &state,
        wire::Record::ServiceUiRequest(wire::ServiceUiRequest { request: 1 }),
    );
    ui.ask(
        &host,
        Question::text(&title, &text).unwrap(),
        &mut input,
        &mut out,
    )
    .unwrap();
    assert_eq!(state.calls.get(), 1);
}

#[test]
fn text_parts_check_identity_offsets_total_and_progress() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let ui = Ui::open(&host).unwrap();
    let mut input = [0; 4096];
    let mut out = [0; 4096];
    let id = RequestId::new(4).unwrap().text();
    let text = "界".repeat(1024);
    set_reply(
        &state,
        wire::Record::ServiceUiText(wire::ServiceUiText {
            text: 4,
            offset: 0,
            total: 3072,
            flags: 1,
            data: &text,
        }),
    );
    let part = ui.text_read(&host, id, 0, &mut input, &mut out).unwrap();
    assert_eq!(part.data(), text);
    assert_eq!(part.next_offset(), None);
    for (actual, offset, total, data) in [
        (5, 0, 1, "x"),
        (4, 1, 2, "x"),
        (4, 0, 0, "x"),
        (4, 0, 2, ""),
    ] {
        set_reply(
            &state,
            wire::Record::ServiceUiText(wire::ServiceUiText {
                text: actual,
                offset,
                total,
                flags: 0,
                data,
            }),
        );
        let expected = if actual != 4 || offset != 0 {
            Error::State
        } else {
            Error::Invalid
        };
        assert_eq!(
            ui.text_read(&host, id, 0, &mut input, &mut out)
                .unwrap_err(),
            expected
        );
    }
    state.reply.borrow_mut().clear();
    ui.text_write(&host, id, 0, &text, TextWrite::Last, &mut input, &mut out)
        .unwrap();
}

unsafe extern "C" fn compose(
    ctx: *mut c_void,
    kind: u32,
    handle: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> i32 {
    // SAFETY: this fixture keeps State alive for the callback.
    let state = unsafe { &*ctx.cast::<State>() };
    state.composed.set(Some((
        kind,
        handle,
        paperpaw_platform::Rect::new(x, y, width, height),
    )));
    state.compose_result.get()
}
unsafe extern "C" fn submit(ctx: *mut c_void, bytes: PpBytes) -> i32 {
    // SAFETY: this fixture keeps State and the encoded input slice alive.
    let state = unsafe { &*ctx.cast::<State>() };
    // SAFETY: the command sender supplies its initialized exclusive scratch prefix.
    state
        .input
        .replace(unsafe { core::slice::from_raw_parts(bytes.ptr, bytes.len) }.to_vec());
    state.result.get()
}
fn set_stream(state: &State, records: &[wire::Record<'_>]) {
    let mut bytes = vec![0; 4096];
    let mut used = 0;
    for record in records {
        used += wire::record_encode_one(record, &mut bytes[used..]).unwrap();
    }
    bytes.truncate(used);
    state.reply.replace(bytes);
}
mod paging;
mod params;
mod stages;
mod widgets;
