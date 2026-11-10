#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::*;
use crate::native::Host;
use alloc::vec::Vec;
use core::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr,
};
use paperpaw_native::host_glue::{self as abi, PpBytes, PpHost};

#[derive(Default)]
struct Context {
    batches: RefCell<Vec<Vec<u8>>>,
    result: Cell<i32>,
}

unsafe extern "C" fn submit(ctx: *mut c_void, batch: PpBytes) -> i32 {
    // SAFETY: tests retain the context; sender lends initialized batch bytes.
    let (ctx, bytes) = unsafe {
        (
            &*ctx.cast::<Context>(),
            core::slice::from_raw_parts(batch.ptr, batch.len),
        )
    };
    ctx.batches.borrow_mut().push(bytes.to_vec());
    ctx.result.get()
}

fn table() -> PpHost {
    let mut table = abi::host_table();
    table.commands_submit = submit;
    table
}

#[test]
fn a_state_is_sent_whole_with_its_flags_keys_and_argument_batches() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 1024];
    let mut args = [0; 512];
    let mut sender = CommandSender::new(&host, &mut scratch);
    let label = [Arg::Count(3)];
    let state = ItemState::new()
        .faint()
        .switch(true)
        .keep()
        .label("update.title", &label)
        .badge("update.new", &[]);
    sender
        .set_item_state("update.check", &state, &mut args)
        .unwrap();
    let batches = ctx.batches.borrow();
    let decoded = wire::commands_decode(&batches[0]).unwrap();
    let [
        wire::Command::SetItemState {
            item,
            flags,
            label,
            label_args,
            badge,
            badge_args,
        },
    ] = decoded.iter().collect::<Vec<_>>()[..]
    else {
        panic!("not an item state");
    };
    assert_eq!(item, "update.check");
    assert_eq!(
        flags,
        wire::COMMAND_ITEM_FAINT
            | wire::COMMAND_ITEM_SWITCH
            | wire::COMMAND_ITEM_ON
            | wire::COMMAND_ITEM_KEEP
    );
    assert_eq!((label, badge), ("update.title", "update.new"));
    assert!(!label_args.is_empty(), "the plural selector travels");
    assert!(badge_args.is_empty(), "no arguments is no bytes");
}

#[test]
fn the_default_state_and_a_switch_turned_off_are_sent_as_they_are() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 256];
    let mut sender = CommandSender::new(&host, &mut scratch);
    let mut args = [];
    sender
        .set_item_state("update.check", &ItemState::new(), &mut args)
        .unwrap();
    sender
        .set_item_state(
            "update.check",
            &ItemState::new().switch(true).switch(false),
            &mut args,
        )
        .unwrap();
    let batches = ctx.batches.borrow();
    let flags: Vec<u32> = batches
        .iter()
        .map(
            |bytes| match wire::commands_decode(bytes).unwrap().iter().next() {
                Some(wire::Command::SetItemState { flags, .. }) => flags,
                _ => panic!("not an item state"),
            },
        )
        .collect();
    assert_eq!(flags, [0, wire::COMMAND_ITEM_SWITCH]);
}

#[test]
fn bad_items_keys_and_arguments_are_refused_before_the_host_is_called() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 1024];
    let mut args = [0; 512];
    let long_item = alloc::string::String::from_utf8(alloc::vec![b'x'; 97]).unwrap();
    let long_key = alloc::string::String::from_utf8(alloc::vec![b'k'; 129]).unwrap();
    let count = [Arg::Count(1)];
    let twice = [
        Arg::Int {
            name: "n",
            value: 1,
        },
        Arg::Int {
            name: "n",
            value: 2,
        },
    ];
    let many = [Arg::Count(1); 17];
    let mut sender = CommandSender::new(&host, &mut scratch);
    for (item, state, error) in [
        ("", ItemState::new(), Error::Invalid),
        (long_item.as_str(), ItemState::new(), Error::Invalid),
        (
            "a",
            ItemState::new().label(long_key.as_str(), &[]),
            Error::Invalid,
        ),
        (
            "a",
            ItemState::new().badge(long_key.as_str(), &[]),
            Error::Invalid,
        ),
        // Arguments without a key.
        ("a", ItemState::new().label("", &count), Error::Invalid),
        ("a", ItemState::new().badge("", &count), Error::Invalid),
        // Duplicate names, too many arguments.
        ("a", ItemState::new().label("k", &twice), Error::Invalid),
        ("a", ItemState::new().badge("k", &many), Error::NoSpace),
    ] {
        assert_eq!(
            sender.set_item_state(item, &state, &mut args),
            Err(error),
            "{item:?}"
        );
    }
    // Scratch too short for the arguments.
    assert_eq!(
        sender.set_item_state("a", &ItemState::new().label("k", &count), &mut []),
        Err(Error::NoSpace)
    );
    assert!(ctx.batches.borrow().is_empty());
}

#[test]
fn the_hosts_refusal_is_the_calls_status_and_a_short_command_scratch_sends_nothing() {
    let table = table();
    let mut ctx = Context::default();
    ctx.result.set(abi::ERR_NOT_FOUND);
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 256];
    let mut args = [0; 16];
    assert_eq!(
        CommandSender::new(&host, &mut scratch).set_item_state(
            "gone",
            &ItemState::new().hidden(),
            &mut args
        ),
        Err(Error::NotFound)
    );
    let mut short = [0; 2];
    let calls = ctx.batches.borrow().len();
    assert_eq!(
        CommandSender::new(&host, &mut short).set_item_state(
            "a",
            &ItemState::new().hidden(),
            &mut args
        ),
        Err(Error::NoSpace)
    );
    assert_eq!(ctx.batches.borrow().len(), calls);
}

#[test]
fn label_and_badge_arguments_both_fit_the_documented_512_byte_scratch() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 256];
    let mut args = [0; 512];
    let label = [
        Arg::Count(2),
        Arg::Text {
            name: "who",
            value: "me",
        },
    ];
    let badge = [Arg::Int {
        name: "n",
        value: 7,
    }];
    CommandSender::new(&host, &mut scratch)
        .set_item_state(
            "update.check",
            &ItemState::new()
                .label("a.title", &label)
                .badge("a.badge", &badge),
            &mut args,
        )
        .unwrap();
    let batches = ctx.batches.borrow();
    let decoded = wire::commands_decode(&batches[0]).unwrap();
    let Some(wire::Command::SetItemState {
        label_args,
        badge_args,
        ..
    }) = decoded.iter().next()
    else {
        panic!("not an item state");
    };
    assert!(!label_args.is_empty() && !badge_args.is_empty());
    // A batch over 256 bytes of one list is NoSpace, not a cut one.
    drop(batches);
    let text = "x".repeat(200);
    let big = [
        Arg::Text {
            name: "a",
            value: &text,
        },
        Arg::Text {
            name: "b",
            value: &text,
        },
    ];
    assert_eq!(
        CommandSender::new(&host, &mut scratch).set_item_state(
            "update.check",
            &ItemState::new().label("a.title", &big),
            &mut args
        ),
        Err(Error::NoSpace)
    );
}
