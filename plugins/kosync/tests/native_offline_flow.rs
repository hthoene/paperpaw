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
