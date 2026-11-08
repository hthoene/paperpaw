//! The scripted host against a small hand-written native plugin: every
//! behaviour a port's test relies on, shown once here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture failures fail the test"
)]
use core::ffi::c_void;
use paperpaw_core::native_host::commands::{WireCommand, commands_encode};
use paperpaw_core::native_host::environment::SettingValueRef;
use paperpaw_core::native_host::registrations::{EventAnswered, Record};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_FULL, ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_STATE, ERR_UNSUPPORTED, OK,
    PpBuffer, PpBytes, PpEntry, PpHost, PpPlugin,
};
use paperpaw_scripted_host::{ScriptedHost, Slot, event_batch, sweep};
use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::{Command, SettingSpec, SettingValue};

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
        // Open a service, call op 7 with "ask", copy the reply to `out`;
        // `arg == 1` forgets to release the handle.
        4 => {
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
                let got = (h.service_call)(
                    fake.ctx,
                    handle,
                    7,
                    PpBytes {
                        ptr: ask.as_ptr(),
                        len: ask.len(),
                    },
                    out,
                );
                if arg != 1 {
                    assert_eq!((h.handle_release)(fake.ctx, handle), OK);
                }
                got
            }
        }
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
        // Read `probe.key` into 4 bytes: the answer is the full length.
        6 => {
            let mut small = [0_u8; 4];
            let key = b"probe.key";
            // SAFETY: scripted table; pointers live for the call.
            unsafe {
                (h.settings_read)(
                    fake.ctx,
                    PpBytes {
                        ptr: key.as_ptr(),
                        len: key.len(),
                    },
                    PpBuffer {
                        ptr: small.as_mut_ptr(),
                        cap: small.len(),
                    },
                )
            }
        }
        // Submit the input as a registration batch.
        7 => {
            // SAFETY: scripted table; `input` is readable.
            unsafe { (h.registrations_submit)(fake.ctx, input) }
        }
        // Echo the first bytes of the `Answered` event's batch length.
        8 => i32::try_from(input.len).unwrap(),
        _ => {
            let _ = out;
            ERR_UNSUPPORTED
        }
    }
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
        input: None,
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
    let done = plugin.event(1, &[3], 0).unwrap();
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
    let ok = plugin.event(1, &[64], 0).unwrap();
    assert_eq!((ok.status, ok.commands.len()), (OK, 64));
    let over = plugin.event(1, &[65], 0).unwrap();
    // The plugin reports the refusal; a negative status drops the whole stage.
    assert_eq!(over.status, ERR_NO_SPACE);
    assert_eq!((over.commands.len(), over.dropped), (0, 64));
    // The limits are the host's to set.
    host.limits(2, 4096);
    let two = plugin.event(1, &[2], 0).unwrap();
    assert_eq!(two.commands.len(), 2);
    assert_eq!(plugin.event(1, &[3], 0).unwrap().status, ERR_NO_SPACE);
}

#[test]
fn a_callback_that_fails_loses_everything_it_staged() {
    let host = host_with_settings();
    let plugin = host.load(&entry()).unwrap();
    let failed = plugin.event(2, &[], 0).unwrap();
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
        let done = plugin.event(1, &[3], 0).unwrap();
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
    let plugin = host.load(&entry()).unwrap();
    let done = plugin.event(4, &[0], 16).unwrap();
    assert_eq!((done.status, done.out.as_slice()), (5, &b"reply"[..]));
    assert_eq!(host.open_handles(), 0);
    // A plugin that forgets to release its handle is caught.
    let leaked = plugin.event(4, &[1], 16).unwrap();
    assert_eq!(leaked.status, 5);
    assert_eq!(host.open_handles(), 1);
    // A reply that does not fit the buffer is ERR_NO_SPACE.
    assert_eq!(plugin.event(4, &[0], 3).unwrap().status, ERR_NO_SPACE);
    // A service the host does not have is unsupported, a version it lacks too.
    let none = ScriptedHost::new();
    assert_eq!(
        none.load(&entry())
            .unwrap()
            .event(4, &[0], 16)
            .unwrap()
            .status,
        ERR_UNSUPPORTED
    );
}

#[test]
fn a_refused_allocation_and_the_sweep_over_every_call() {
    let host = ScriptedHost::new();
    host.refuse_nth(Slot::Heap, 1, ERR_NO_MEMORY);
    let plugin = host.load(&entry()).unwrap();
    assert_eq!(plugin.event(5, &[], 0).unwrap().status, ERR_NO_MEMORY);
    drop(plugin);
    assert_eq!(host.live_allocations(), 0);
    // One clean run plus one run per allocation the plugin made (here: one).
    let runs = sweep(Slot::Heap, ERR_NO_MEMORY, |host, refused| {
        let plugin = host.load(&entry()).unwrap();
        let status = plugin.event(5, &[], 0).unwrap().status;
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
    assert_eq!(plugin.event(6, &[], 0).unwrap().status, 8);
    host.set_value("probe.key", b"xy");
    assert_eq!(plugin.event(6, &[], 0).unwrap().status, 2);
}

#[test]
fn registrations_and_events_as_batches() {
    let host = ScriptedHost::new();
    let plugin = host.load(&entry()).unwrap();
    assert_eq!(plugin.event(7, &[0], 0).unwrap().status, OK);
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
    assert_eq!(
        plugin.event(8, &answered, 0).unwrap().status,
        i32::try_from(answered.len()).unwrap()
    );
}
