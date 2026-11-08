//! The scripted host against a small hand-written native plugin: every
//! behaviour a port's test relies on, shown once here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture failures fail the test"
)]
use core::cell::RefCell;
use core::ffi::c_void;
use paperpaw_core::native_host::commands::{WireCommand, commands_encode};
use paperpaw_core::native_host::environment::{SettingValueRef, settings_decode};
use paperpaw_core::native_host::registrations::{EventAnswered, Record};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_FULL, ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE,
    ERR_UNSUPPORTED, OK, PpBuffer, PpBytes, PpEntry, PpHost, PpPlugin,
};
use paperpaw_scripted_host::{ScriptedHost, Slot, event_batch, sweep};
use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::{Command, SettingSpec, SettingValue};

thread_local! {
    /// What the last scenario callback saw: events return a status only, so the
    /// fake plugin leaves its observations here for the test to read.
    static SEEN: RefCell<(i32, Vec<u8>)> = const { RefCell::new((0, Vec::new())) };
}

fn seen() -> (i32, Vec<u8>) {
    SEEN.with(|seen| seen.take())
}

fn note(status: i32, bytes: &[u8]) {
    SEEN.with(|seen| *seen.borrow_mut() = (status, bytes.to_vec()));
}

/// The fake plugin's state: what the host gave `make`.
struct Fake {
    host: *const PpHost,
    ctx: *mut c_void,
}

fn host(fake: &Fake) -> &PpHost {
    // SAFETY: the host table outlives the plugin.
    unsafe { &*fake.host }
}

/// Stages one `probe.flag` command in its own batch.
fn stage_flag(fake: &Fake) -> i32 {
    let mut bytes = [0_u8; 256];
    let len = commands_encode(
        &[WireCommand::SetSetting {
            key: "probe.flag",
            value: SettingValueRef::Bool(true),
        }],
        &mut bytes,
    )
    .unwrap();
    // SAFETY: the host table is the scripted one; `bytes` is readable.
    unsafe {
        (host(fake).commands_submit)(
            fake.ctx,
            PpBytes {
                ptr: bytes.as_ptr(),
                len,
            },
        )
    }
}

/// Token 4: see the event callback.
fn open_and_call(fake: &Fake, arg: u8) -> i32 {
    let h = host(fake);
    {
        let mut handle = 0_u32;
        let key = b"core.test";
        // SAFETY: scripted table; pointers live for the calls.
        unsafe {
            let status = (h.service_open)(
                fake.ctx,
                PpBytes {
                    ptr: key.as_ptr(),
                    len: key.len(),
                },
                1,
                0,
                &raw mut handle,
            );
            if status != OK {
                return status;
            }
            let ask = b"ask";
            let mut reply = [0_u8; 16];
            let cap = if arg & 2 == 0 { reply.len() } else { 3 };
            let got = (h.service_call)(
                fake.ctx,
                handle,
                7,
                PpBytes {
                    ptr: ask.as_ptr(),
                    len: ask.len(),
                },
                PpBuffer {
                    ptr: reply.as_mut_ptr(),
                    cap,
                },
            );
            note(got, &reply[..usize::try_from(got).unwrap_or(0).min(cap)]);
            if arg & 1 == 0 {
                assert_eq!((h.handle_release)(fake.ctx, handle), OK);
            }
            OK
        }
    }
}

/// Token 6: see the event callback.
fn read_setting(fake: &Fake, arg: u8) -> i32 {
    let h = host(fake);
    {
        let mut buffer = [0_u8; 32];
        let cap = if arg == 1 { buffer.len() } else { 4 };
        let key = b"probe.key";
        // SAFETY: scripted table; pointers live for the call.
        let got = unsafe {
            (h.settings_read)(
                fake.ctx,
                PpBytes {
                    ptr: key.as_ptr(),
                    len: key.len(),
                },
                PpBuffer {
                    ptr: buffer.as_mut_ptr(),
                    cap,
                },
            )
        };
        note(got, &buffer[..usize::try_from(got).unwrap_or(0).min(cap)]);
        OK
    }
}

unsafe extern "C" fn event(state: *mut c_void, token: u32, input: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: `state` is the leaked `Fake` below.
    let fake = unsafe { &*state.cast::<Fake>() };
    let arg = if input.len == 0 {
        0
    } else {
        // SAFETY: readable input of `len` bytes.
        unsafe { *input.ptr }
    };
    let h = host(fake);
    match token {
        // Stage `arg` commands; stop at the first refusal and report it.
        1 => {
            for _ in 0..arg {
                let status = stage_flag(fake);
                if status != OK {
                    return status;
                }
            }
            OK
        }
        // Stage three, then fail: the host drops them all.
        2 => {
            for _ in 0..3 {
                assert_eq!(stage_flag(fake), OK);
            }
            ERR_INVALID
        }
        // Open a service, call op 7 with "ask" into a 16 byte buffer (3 bytes
        // when `arg & 2`) and note the call's status and reply; `arg & 1`
        // forgets to release the handle. Returns the open's status.
        4 => open_and_call(fake, arg),
        // One heap block, freed at once.
        5 => {
            let mut block: *mut c_void = core::ptr::null_mut();
            // SAFETY: scripted heap.
            unsafe {
                let status = (h.heap_alloc)(fake.ctx, 64, 8, &raw mut block);
                if status != OK {
                    return status;
                }
                (h.heap_dealloc)(fake.ctx, block, 64, 8)
            }
        }
        // Read `probe.key` into 4 bytes (32 bytes when `arg == 1`) and note the
        // answer, which is the full length.
        6 => read_setting(fake, arg),
        // Submit the input as a registration batch.
        7 => {
            // SAFETY: scripted table; `input` is readable.
            unsafe { (h.registrations_submit)(fake.ctx, input) }
        }
        // Note the input's length; a plugin that returns a length is at fault.
        8 => {
            note(i32::try_from(input.len).unwrap(), &[]);
            i32::try_from(input.len).unwrap()
        }
        // Note the capacity of the reply buffer an event is given.
        9 => {
            note(i32::try_from(out.cap).unwrap(), &[]);
            OK
        }
        _ => ERR_UNSUPPORTED,
    }
}

/// The input callback: its status is the token (so a test chooses it).
unsafe extern "C" fn input(_state: *mut c_void, token: u32, _input: PpBytes) -> i32 {
    // The token is the status as an unsigned bit pattern.
    i32::from_ne_bytes(token.to_ne_bytes())
}

unsafe extern "C" fn draw(state: *mut c_void, _token: u32, _input: PpBytes) -> i32 {
    // SAFETY: `state` is the leaked `Fake`.
    let fake = unsafe { &*state.cast::<Fake>() };
    // A draw has no stage: the submit's own status is the draw's.
    stage_flag(fake)
}

unsafe extern "C" fn plugin_drop(state: *mut c_void) {
    // SAFETY: `state` is the box leaked by `make`; dropped once.
    drop(unsafe { Box::from_raw(state.cast::<Fake>()) });
}

unsafe extern "C" fn make(host: *const PpHost, ctx: *mut c_void, out: *mut *const PpPlugin) -> i32 {
    let state = Box::into_raw(Box::new(Fake { host, ctx }));
    let table = Box::new(PpPlugin {
        size: core::mem::size_of::<PpPlugin>() as u32,
        state: state.cast(),
        drop: Some(plugin_drop),
        dispatch_event: Some(event),
        draw: Some(draw),
        input: Some(input),
    });
    // SAFETY: `out` is the host's writable slot; the table is leaked and
    // released together with its state by `plugin_drop`'s owner (the test
    // process), which is fine for a fixture.
    unsafe { *out = Box::into_raw(table) };
    OK
}

fn entry() -> PpEntry {
    PpEntry {
        size: core::mem::size_of::<PpEntry>() as u32,
        abi_major: 7,
        needs_minor: 1,
        metadata: PpBytes {
            ptr: core::ptr::null(),
            len: 0,
        },
        make: Some(make),
    }
}

fn host_with_settings() -> ScriptedHost {
    let mut host = ScriptedHost::new();
    host.settings_mut()
        .define(SettingSpec::switch(
            "probe.flag",
            "flag",
            false,
            groups::OTHER,
        ))
        .unwrap();
    host
}

#[test]
fn staged_commands_are_typed_and_in_order() {
    let host = host_with_settings();
    let plugin = host.load(&entry()).unwrap();
    let done = plugin.event(1, &[3]).unwrap();
    assert_eq!(done.status, OK);
    assert_eq!(done.commands.len(), 3);
    assert!(matches!(
        &done.commands[0],
        Command::SetSetting(id, SettingValue::Bool(true)) if id.as_str() == "probe.flag"
    ));
    assert_eq!(host.calls(Slot::Commands), 3);
    // The wire bytes of each batch are there for exact assertions.
    assert_eq!(done.batches.len(), 3);
    assert!(done.batches[0].starts_with(&[1]));
}

#[test]
fn the_stage_takes_64_commands_and_refuses_the_65th_with_nothing_more_staged() {
    let host = host_with_settings();
    let plugin = host.load(&entry()).unwrap();
    let ok = plugin.event(1, &[64]).unwrap();
    assert_eq!((ok.status, ok.commands.len()), (OK, 64));
    let over = plugin.event(1, &[65]).unwrap();
    // The plugin reports the refusal; a negative status drops the whole stage.
    assert_eq!(over.status, ERR_NO_SPACE);
    assert_eq!((over.commands.len(), over.dropped), (0, 64));
    // The limits are the host's to set.
    host.limits(2, 4096);
    let two = plugin.event(1, &[2]).unwrap();
    assert_eq!(two.commands.len(), 2);
    assert_eq!(plugin.event(1, &[3]).unwrap().status, ERR_NO_SPACE);
}

#[test]
fn a_callback_that_fails_loses_everything_it_staged() {
    let host = host_with_settings();
    let plugin = host.load(&entry()).unwrap();
    let failed = plugin.event(2, &[]).unwrap();
    assert_eq!(failed.status, ERR_INVALID);
    assert!(failed.commands.is_empty());
    assert_eq!(failed.dropped, 3);
}

#[test]
fn a_draw_has_no_stage() {
    let host = host_with_settings();
    let plugin = host.load(&entry()).unwrap();
    assert_eq!(plugin.draw(0, &[]).unwrap().status, ERR_STATE);
}

#[test]
fn a_refusal_at_the_nth_commands_call_is_the_plugins_status() {
    for status in [ERR_FULL, ERR_BUSY] {
        let host = host_with_settings();
        host.refuse_nth(Slot::Commands, 2, status);
        let plugin = host.load(&entry()).unwrap();
        let done = plugin.event(1, &[3]).unwrap();
        assert_eq!(done.status, status);
        assert_eq!((done.commands.len(), done.dropped), (0, 1));
    }
}

#[test]
fn scripted_service_answers_reach_the_plugin_and_handles_are_counted() {
    let host = ScriptedHost::new();
    host.service("core.test", 1, 0, |op, input| {
        assert_eq!((op, input), (7, &b"ask"[..]));
        Ok(b"reply".to_vec())
    });
    host.reply_bound("core.test", 7, 16);
    let plugin = host.load(&entry()).unwrap();
    assert_eq!(plugin.event(4, &[0]).unwrap().status, OK);
    assert_eq!(seen(), (5, b"reply".to_vec()));
    assert_eq!(host.open_handles(), 0);
    // A plugin that forgets to release its handle is caught.
    assert_eq!(plugin.event(4, &[1]).unwrap().status, OK);
    assert_eq!(host.open_handles(), 1);
}

#[test]
fn a_service_key_the_host_lacks_is_not_found_and_a_version_it_lacks_unsupported() {
    let none = ScriptedHost::new();
    let plugin = none.load(&entry()).unwrap();
    assert_eq!(plugin.event(4, &[0]).unwrap().status, ERR_NOT_FOUND);
    // The key exists, the major does not.
    let other = ScriptedHost::new();
    other.service("core.test", 2, 0, |_, _| Ok(Vec::new()));
    let plugin = other.load(&entry()).unwrap();
    assert_eq!(plugin.event(4, &[0]).unwrap().status, ERR_UNSUPPORTED);
    assert_eq!(other.open_handles(), 0);
}

#[test]
fn a_plugin_holds_one_handle_per_service_key() {
    let host = ScriptedHost::new();
    host.service("core.test", 1, 0, |_, _| Ok(Vec::new()));
    let plugin = host.load(&entry()).unwrap();
    // The first call forgets its handle, so the second open is a second
    // handle for the same key: the shell refuses it with ERR_FULL.
    assert_eq!(plugin.event(4, &[1]).unwrap().status, OK);
    assert_eq!(plugin.event(4, &[1]).unwrap().status, ERR_FULL);
    assert_eq!(host.open_handles(), 1);
}

#[test]
fn a_call_checks_the_buffer_before_the_handler_runs() {
    let ran = std::rc::Rc::new(core::cell::Cell::new(0_usize));
    let host = ScriptedHost::new();
    let count = ran.clone();
    host.service("core.test", 1, 0, move |_, _| {
        count.set(count.get() + 1);
        Ok(b"reply".to_vec())
    });
    host.reply_bound("core.test", 7, 16);
    let plugin = host.load(&entry()).unwrap();
    // The plugin's 3 byte buffer is below the op's bound of 16: no handler
    // run, nothing written.
    plugin.event(4, &[2]).unwrap();
    assert_eq!(seen(), (ERR_NO_SPACE, Vec::new()));
    assert_eq!(ran.get(), 0);
    // A fitting buffer runs it once.
    plugin.event(4, &[0]).unwrap();
    assert_eq!(seen(), (5, b"reply".to_vec()));
    assert_eq!(ran.get(), 1);
}

#[test]
fn a_reply_over_the_ops_bound_is_the_services_fault() {
    let host = ScriptedHost::new();
    host.service("core.test", 1, 0, |_, _| Ok(vec![7; 9]));
    host.reply_bound("core.test", 7, 8);
    let plugin = host.load(&entry()).unwrap();
    plugin.event(4, &[0]).unwrap();
    // The 16 byte buffer holds the bound; the 9 byte reply exceeds it.
    assert_eq!(seen().0, ERR_INVALID);
}

#[test]
fn events_get_no_reply_buffer_and_a_positive_status_is_a_fault() {
    let host = ScriptedHost::new();
    let plugin = host.load(&entry()).unwrap();
    assert_eq!(plugin.event(9, &[]).unwrap().status, OK);
    assert_eq!(seen().0, 0, "the event's reply buffer has no capacity");
    // A positive result is never a length: it is ERR_INVALID.
    let answered = event_batch(&Record::EventAnswered(EventAnswered {
        now_ms: 0,
        request: 1,
        kind: 0,
        outcome: 0,
        choice: 1,
        length: 0,
        flags: 0,
    }))
    .unwrap();
    assert_eq!(plugin.event(8, &answered).unwrap().status, ERR_INVALID);
    assert_eq!(seen().0, i32::try_from(answered.len()).unwrap());
}

#[test]
fn input_takes_zero_or_one_and_anything_above_is_a_fault() {
    let host = ScriptedHost::new();
    let plugin = host.load(&entry()).unwrap();
    for (token, status) in [(0_u32, 0), (1, 1), (2, ERR_INVALID), (200, ERR_INVALID)] {
        assert_eq!(plugin.input(token, &[]).unwrap().status, status);
    }
    // An error status is the plugin's own.
    let token = u32::from_ne_bytes(ERR_BUSY.to_ne_bytes());
    assert_eq!(plugin.input(token, &[]).unwrap().status, ERR_BUSY);
}

#[test]
fn a_refused_allocation_and_the_sweep_over_every_call() {
    let host = ScriptedHost::new();
    host.refuse_nth(Slot::Heap, 1, ERR_NO_MEMORY);
    let plugin = host.load(&entry()).unwrap();
    assert_eq!(plugin.event(5, &[]).unwrap().status, ERR_NO_MEMORY);
    drop(plugin);
    assert_eq!(host.live_allocations(), 0);
    // One clean run plus one run per allocation the plugin made (here: one).
    let runs = sweep(Slot::Heap, ERR_NO_MEMORY, |host, refused| {
        let plugin = host.load(&entry()).unwrap();
        let status = plugin.event(5, &[]).unwrap().status;
        assert_eq!(status, if refused.is_some() { ERR_NO_MEMORY } else { OK });
        status
    });
    assert_eq!(runs.len(), 2);
}

#[test]
fn settings_read_answers_the_full_length_and_follows_changes() {
    let host = ScriptedHost::new();
    host.set_value("probe.key", b"abcdefgh");
    let plugin = host.load(&entry()).unwrap();
    plugin.event(6, &[]).unwrap();
    assert_eq!(seen(), (8, b"abcd".to_vec()));
    host.set_value("probe.key", b"xy");
    plugin.event(6, &[]).unwrap();
    assert_eq!(seen(), (2, b"xy".to_vec()));
}

#[test]
fn a_setting_without_a_value_reads_as_the_encoded_absent_record() {
    let host = ScriptedHost::new();
    let plugin = host.load(&entry()).unwrap();
    plugin.event(6, &[1]).unwrap();
    let (status, record) = seen();
    // 12 bytes: never ERR_NOT_FOUND. The record decodes as Absent, which is
    // not false, zero or empty text.
    assert_eq!(status, 12);
    assert_eq!(settings_decode(&record), Ok(SettingValueRef::Absent));
    host.set_setting("probe.key", SettingValueRef::Bool(false));
    plugin.event(6, &[1]).unwrap();
    assert_eq!(settings_decode(&seen().1), Ok(SettingValueRef::Bool(false)));
}

#[test]
fn registrations_and_events_as_batches() {
    let host = ScriptedHost::new();
    let plugin = host.load(&entry()).unwrap();
    assert_eq!(plugin.event(7, &[0]).unwrap().status, OK);
    assert_eq!(host.registrations(), [std::vec![0_u8]]);
    // An `Answered` event reaches the plugin as one record.
    let answered = event_batch(&Record::EventAnswered(EventAnswered {
        now_ms: 0,
        request: 1,
        kind: 0,
        outcome: 0,
        choice: 1,
        length: 0,
        flags: 0,
    }))
    .unwrap();
    // The plugin sees it as one record in its batch (its length is noted).
    assert_eq!(plugin.event(8, &answered).unwrap().status, ERR_INVALID);
    assert_eq!(seen().0, i32::try_from(answered.len()).unwrap());
}
