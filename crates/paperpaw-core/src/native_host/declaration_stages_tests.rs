//! Stages and subscriptions in the declaration store: installed with their
//! plugin, found by full id, refused past eight, gone with the plugin.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
extern crate std;
use super::super::{Allocations, DeclarationStore, ImageOwner, PendingDeclarations};
use super::*;
use crate::native_host::registrations::{
    REGISTRATION_MAX_BYTES, RegistrationStage, StageSpec, records_encode,
};
use alloc::{rc::Rc, vec, vec::Vec};
use core::cell::Cell;
use paperpaw_native::host_glue::{ERR_FULL, ERR_NO_MEMORY};
use paperpaw_sdk::Id;

/// A plugin that is there: a host owner id and a generation.
struct Plugin {
    id: &'static str,
    host: u64,
    live: Cell<bool>,
}
impl ImageOwner for Plugin {
    fn plugin_id(&self) -> &str {
        self.id
    }
    fn generation(&self) -> u32 {
        u32::from(self.live.get())
    }
    fn owner_id(&self) -> Result<u64, i32> {
        Ok(self.host)
    }
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        visit(self.live.get().then_some(&[][..]));
    }
    fn invalid_declaration(&self, _: i32) {}
}

fn plugin(id: &'static str, host: u64) -> Rc<Plugin> {
    Rc::new(Plugin {
        id,
        host,
        live: Cell::new(true),
    })
}

fn spec(name: &str) -> Record<'_> {
    Record::StageSpec(StageSpec {
        name,
        schema: 3,
        flags: 0b101,
        max_input: 100,
        max_reply: 200,
        max_records: 4,
    })
}

fn subscribe(stage: &str, token: u32, order: i32) -> Record<'_> {
    Record::RegistrationStage(RegistrationStage {
        stage,
        token,
        order,
        signature: u64::from(token) * 10,
    })
}

fn batch(records: &[Record<'_>]) -> Vec<u8> {
    let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
    let len = records_encode(records, &mut bytes).unwrap();
    bytes.truncate(len);
    bytes
}

fn install(
    store: &mut DeclarationStore,
    plugin: &Rc<Plugin>,
    batches: &[Vec<u8>],
    allocations: &mut Allocations,
) -> Result<(), i32> {
    let mut pending = PendingDeclarations::new(Id::try_parse(plugin.id).unwrap(), plugin.clone());
    for bytes in batches {
        pending.stage_batch(bytes, allocations)?;
    }
    store.prepare(pending, allocations)?.commit();
    Ok(())
}

fn none() -> Vec<(u64, u32, i16)> {
    Vec::new()
}

fn subscribers(store: &DeclarationStore, stage: &str) -> Vec<(u64, u32, i16)> {
    let mut found: Vec<_> = store
        .stage_subscribers(stage)
        .unwrap()
        .into_iter()
        .map(|s| (s.owner, s.token, s.order))
        .collect();
    found.sort_unstable();
    found
}

#[test]
fn a_stage_and_its_subscriptions_install_with_their_plugin_and_go_with_it() {
    let (reader, notes, about) = (plugin("reader", 1), plugin("notes", 2), plugin("about", 3));
    let mut store = DeclarationStore::default();
    let mut allocations = Allocations::default();
    install(
        &mut store,
        &reader,
        &[batch(&[spec("insert"), spec("after")])],
        &mut allocations,
    )
    .unwrap();
    install(
        &mut store,
        &notes,
        &[batch(&[
            subscribe("reader.insert", 7, -3),
            subscribe("reader.insert", 8, 5),
            subscribe("reader.after", 7, 0),
        ])],
        &mut allocations,
    )
    .unwrap();
    // A subscription to a stage nobody has installed is kept, and inert.
    install(
        &mut store,
        &about,
        &[batch(&[subscribe("lookup.entry", 1, 0)])],
        &mut allocations,
    )
    .unwrap();

    let limits = store.stage("reader.insert").unwrap();
    assert_eq!(
        limits,
        StageLimits {
            owner: 1,
            schema: 3,
            flags: 0b101,
            max_input: 100,
            max_reply: 200,
            max_records: 4,
        }
    );
    assert!(store.stage("reader.after").is_some());
    assert_eq!(store.stage("reader.missing"), None);
    assert_eq!(store.stage("notes.insert"), None);
    assert_eq!(store.stage("reader"), None);

    assert_eq!(
        subscribers(&store, "reader.insert"),
        [(2, 7, -3), (2, 8, 5)]
    );
    assert_eq!(subscribers(&store, "reader.after"), [(2, 7, 0)]);
    assert_eq!(subscribers(&store, "lookup.entry"), [(3, 1, 0)]);
    assert_eq!(subscribers(&store, "reader.missing"), none());
    // In declaration order, for the broker's tie-break.
    let mut index = store
        .stage_subscribers("reader.insert")
        .unwrap()
        .into_iter()
        .map(|s| (s.token, s.index))
        .collect::<Vec<_>>();
    index.sort_unstable();
    assert!(index[0].1 < index[1].1);

    // The subscriber goes: its subscriptions with it, the stage stays.
    store.remove_host_owner(2);
    assert_eq!(subscribers(&store, "reader.insert"), none());
    assert!(store.stage("reader.insert").is_some());
    // The owner goes: the stage with it.
    store.remove_host_owner(1);
    assert_eq!(store.stage("reader.insert"), None);
    // A plugin that stopped (its generation is gone) is absent at once.
    about.live.set(false);
    assert_eq!(subscribers(&store, "lookup.entry"), none());
}

#[test]
fn a_ninth_subscription_refuses_the_whole_plugin_and_changes_nothing() {
    let (reader, notes) = (plugin("reader", 1), plugin("notes", 2));
    let mut store = DeclarationStore::default();
    let mut allocations = Allocations::default();
    install(
        &mut store,
        &reader,
        &[batch(&[spec("insert")])],
        &mut allocations,
    )
    .unwrap();
    let eight: Vec<_> = (0..8)
        .map(|token| subscribe("reader.insert", token, 0))
        .collect();
    // Eight in one batch, or spread over two, are fine.
    let two = [batch(&eight[..5]), batch(&eight[5..])];
    install(&mut store, &notes, &two, &mut allocations).unwrap();
    assert_eq!(subscribers(&store, "reader.insert").len(), 8);
    store.remove_host_owner(2);

    // Nine over two batches: refused for the whole plugin.
    let nine = [
        batch(&eight[..5]),
        batch(&[
            eight[5],
            eight[6],
            eight[7],
            subscribe("reader.insert", 9, 0),
        ]),
    ];
    assert_eq!(
        install(&mut store, &notes, &nine, &mut allocations),
        Err(ERR_FULL)
    );
    assert_eq!(subscribers(&store, "reader.insert"), none());
}

#[test]
fn memory_refused_at_any_reservation_installs_nothing() {
    let (reader, notes) = (plugin("reader", 1), plugin("notes", 2));
    let records = [
        batch(&[spec("insert")]),
        batch(&[
            subscribe("reader.insert", 1, 0),
            subscribe("reader.insert", 2, 0),
        ]),
    ];
    let mut done = false;
    for fail_after in 0..16 {
        let mut store = DeclarationStore::default();
        let mut allocations = Allocations {
            fail_after: Some(fail_after),
            ..Default::default()
        };
        let result = install(&mut store, &reader, &records[..1], &mut allocations)
            .and_then(|()| install(&mut store, &notes, &records[1..], &mut allocations));
        match result {
            Err(error) => {
                assert_eq!(error, ERR_NO_MEMORY);
                // Whatever was refused left no subscription behind.
                assert_eq!(subscribers(&store, "reader.insert"), none());
            }
            Ok(()) => {
                assert_eq!(subscribers(&store, "reader.insert").len(), 2);
                assert!(store.stage("reader.insert").is_some());
                done = true;
                break;
            }
        }
    }
    assert!(done);
}

#[test]
fn plugin_ids_with_dots_split_the_stage_id_at_the_last_dot() {
    let (dotted, plain) = (plugin("a.b", 4), plugin("a", 5));
    let mut store = DeclarationStore::default();
    let mut allocations = Allocations::default();
    install(
        &mut store,
        &dotted,
        &[batch(&[spec("c")])],
        &mut allocations,
    )
    .unwrap();
    install(&mut store, &plain, &[batch(&[spec("b")])], &mut allocations).unwrap();
    // `a.b.c` is `a.b`'s stage `c`, never `a`'s `b.c`; `a.b` is `a`'s `b`.
    assert_eq!(store.stage("a.b.c").unwrap().owner, 4);
    assert_eq!(store.stage("a.b").unwrap().owner, 5);
    assert_eq!(store.stage("a.c"), None);
}
