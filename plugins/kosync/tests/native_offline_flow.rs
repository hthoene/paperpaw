//! The real plugin driven by the scripted host where Offline mode holds a
//! request back that no reader started on the account screen (#459, as Beta
//! 3): the fetch of an opened book ends with `Reason::Offline` (11) and stays
//! quiet, no question and no `command.go_online`. (The account screen's offer
//! is driven on the real host, `native_host.rs`: the scripted host stages no
//! screen redraws, which the account list needs for its input.)
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
mod common;
use common::World;
use paperpaw_core::native_host::commands::{WireCommand, commands_decode};
use paperpaw_core::native_host::registrations::{EventHttpDone, Record};
use paperpaw_plugin_kosync::declarations as tokens;
use paperpaw_scripted_host::{Callback, Plugin, event_batch};

/// The end of request `index`, held back by Offline mode (reason 11).
fn held_back(world: &World, plugin: &Plugin<'_>, index: usize) -> Callback {
    let (id, token) = {
        let server = world.server.borrow();
        (server.requests[index].id, server.requests[index].token)
    };
    let done = event_batch(&Record::EventHttpDone(EventHttpDone {
        now_ms: 100,
        id,
        token,
        status: 0,
        reason: 11,
        flags: 0,
        body_len: 0,
    }))
    .unwrap();
    plugin.event(tokens::HTTP_DONE, &done).unwrap()
}

/// Whether the callback sent `command.go_online` (for 5 minutes).
fn went_online(callback: &Callback) -> bool {
    callback.batches.iter().any(|batch| {
        commands_decode(batch)
            .unwrap()
            .iter()
            .any(|command| matches!(command, WireCommand::GoOnline { for_ms: 300_000 }))
    })
}

#[test]
fn a_fetch_no_reader_started_on_the_account_screen_never_asks() {
    let world = World::new(true);
    let plugin = world.load();
    assert_eq!(world.opened(&plugin, "first.epub", 100_000).status, 0);
    assert_eq!(world.server.borrow().requests.len(), 1);
    let quiet = held_back(&world, &plugin, 0);
    assert_eq!(quiet.status, 0);
    assert!(world.dialogs.questions().is_empty(), "nothing asked");
    assert!(!went_online(&quiet));
    assert_eq!(world.server.borrow().requests.len(), 1);
}

/// A `core.device` whose power mode lets the network be used or not
/// (Offline); the other questions are not answered.
fn power(world: &World) -> std::rc::Rc<std::cell::Cell<bool>> {
    use paperpaw_core::native_host::registrations::ServiceDevPower;
    let permitted = std::rc::Rc::new(std::cell::Cell::new(false));
    let now = permitted.clone();
    world.host.service("core.device", 1, 0, move |op, _| {
        if op != 12 {
            return Err(-1);
        }
        paperpaw_scripted_host::row(&Record::ServiceDevPower(ServiceDevPower {
            level: if now.get() { 0 } else { 2 },
            mode: if now.get() { "normal" } else { "offline" },
            flags: if now.get() { 2 } else { 0 },
            stretch: 1,
            every_ms: 0,
        }))
    });
    world.host.reply_bound("core.device", 12, 512);
    permitted
}

fn allowed(plugin: &Plugin<'_>) -> Callback {
    use paperpaw_core::native_host::registrations::EventWorkAllowed;
    let event = event_batch(&Record::EventWorkAllowed(EventWorkAllowed { now_ms: 200 })).unwrap();
    plugin.event(tokens::WORK_ALLOWED, &event).unwrap()
}

/// Offline mode (#486, as Beta 3): a book opened asks the server nothing and
/// the book left keeps its position; once the network is allowed, the
/// position of the book left is fetched and then sent, and the open book's
/// fetched.
#[test]
fn offline_mode_asks_nothing_and_the_book_left_goes_once_allowed() {
    let world = World::new(true);
    let permitted = power(&world);
    let plugin = world.load();
    assert_eq!(world.opened(&plugin, "first.epub", 100_000).status, 0);
    assert_eq!(world.opened(&plugin, "second.epub", 200_000).status, 0);
    assert!(world.server.borrow().requests.is_empty(), "nothing asked");
    permitted.set(true);
    assert_eq!(allowed(&plugin).status, 0);
    let methods: Vec<String> = world
        .server
        .borrow()
        .requests
        .iter()
        .map(|r| r.method.clone())
        .collect();
    assert_eq!(methods, ["GET", "GET"], "the book left and the open one");
    // The book left: not further elsewhere, its position goes out.
    assert_eq!(world.complete(&plugin, 0, 200, b"{}").status, 0);
    let server = world.server.borrow();
    let push = server.requests.last().unwrap();
    assert_eq!(push.method, "PUT");
    assert!(
        std::str::from_utf8(&push.body)
            .unwrap()
            .contains("\"percentage\":0.1000")
    );
}

/// A push of the book left that fails is not lost: it goes again with the
/// next chance to send (here the open book's close).
#[test]
fn a_failed_push_of_the_book_left_goes_again() {
    let world = World::new(true);
    let plugin = world.load();
    assert_eq!(world.opened(&plugin, "first.epub", 100_000).status, 0);
    assert_eq!(world.complete(&plugin, 0, 200, b"{}").status, 0);
    assert_eq!(world.opened(&plugin, "second.epub", 200_000).status, 0);
    let push = {
        let server = world.server.borrow();
        let at = server
            .requests
            .iter()
            .position(|r| r.method == "PUT")
            .unwrap();
        assert!(
            std::str::from_utf8(&server.requests[at].body)
                .unwrap()
                .contains("\"percentage\":0.1000")
        );
        at
    };
    assert_eq!(world.complete(&plugin, push, 500, b"").status, 0);
    let before = world.server.borrow().requests.len();
    assert_eq!(
        world.closed(&plugin, "second.epub", 300_000, 70_000).status,
        0
    );
    let server = world.server.borrow();
    let again: Vec<&str> = server.requests[before..]
        .iter()
        .map(|r| r.method.as_str())
        .collect();
    assert!(
        again.contains(&"GET"),
        "the book left is fetched again: {again:?}"
    );
}
