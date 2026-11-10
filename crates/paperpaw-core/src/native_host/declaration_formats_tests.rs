//! Formats and readers in the declaration store: installed with their plugin,
//! listed in load order, gone with the plugin, thirty-two on the device.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
extern crate std;
use super::super::{Allocations, DeclarationStore, ImageOwner, PendingDeclarations};
use super::*;
use crate::native_host::declaration_checks::Cap;
use crate::native_host::registrations::{LookupSpec, REGISTRATION_MAX_BYTES, records_encode};
use alloc::{format, rc::Rc, string::String, vec, vec::Vec};
use core::cell::Cell;
use paperpaw_native::host_glue::ERR_FULL;
use paperpaw_sdk::Id;

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

fn format<'a>(name: &'a str, extensions: &'a str, priority: i32) -> Record<'a> {
    Record::FormatSpec(FormatSpec {
        name,
        title: "Title",
        extensions,
        flags: 0,
        priority,
        token: 7,
    })
}

fn reader<'a>(name: &'a str) -> Record<'a> {
    Record::ReaderSpec(ReaderSpec {
        name,
        screen: "x.main",
        formats: "",
        flags: 0,
    })
}

fn lookup<'a>(name: &'a str) -> Record<'a> {
    Record::LookupSpec(LookupSpec {
        name,
        title: "Title",
        languages: "en",
        flags: 0,
        token: 3,
    })
}

fn providers(store: &DeclarationStore) -> Vec<(u64, String)> {
    let mut found = Vec::new();
    store.each_lookup(&mut |l| found.push((l.owner, format!("{}.{}", l.plugin, l.spec.name))));
    found
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
) -> Result<(), i32> {
    install_capped(store, plugin, batches).0
}

/// As `install`, with the cap `prepare` said it refused the plugin at.
fn install_capped(
    store: &mut DeclarationStore,
    plugin: &Rc<Plugin>,
    batches: &[Vec<u8>],
) -> (Result<(), i32>, Option<Cap>) {
    let mut allocations = Allocations::default();
    let mut pending = PendingDeclarations::new(Id::try_parse(plugin.id).unwrap(), plugin.clone());
    for bytes in batches {
        if let Err(error) = pending.stage_batch(bytes, &mut allocations) {
            return (Err(error), allocations.cap);
        }
    }
    match store.prepare(pending, &mut allocations) {
        Ok(prepared) => {
            prepared.commit();
            (Ok(()), None)
        }
        Err(error) => (Err(error), allocations.cap),
    }
}

fn formats(store: &DeclarationStore) -> Vec<(u64, String, i32)> {
    let mut found = Vec::new();
    store.each_format(&mut |f| {
        found.push((
            f.owner,
            format!("{}.{}", f.plugin, f.spec.name),
            f.spec.priority,
        ));
    });
    found
}

#[test]
fn formats_and_readers_are_listed_in_load_order_and_go_with_their_plugin() {
    let (epub, txt, notes) = (plugin("epub", 1), plugin("txt", 2), plugin("notes", 3));
    let mut store = DeclarationStore::default();
    install(&mut store, &txt, &[batch(&[format("txt", "txt", 0)])]).unwrap();
    install(
        &mut store,
        &epub,
        &[batch(&[format("epub", "epub", 5), reader("app")])],
    )
    .unwrap();
    // A plugin without any, and a reader in its own right.
    install(&mut store, &notes, &[batch(&[reader("view")])]).unwrap();

    assert_eq!(
        formats(&store),
        [(2, "txt.txt".into(), 0), (1, "epub.epub".into(), 5),]
    );
    let mut readers = Vec::new();
    store.each_reader(&mut |r| readers.push((String::from(r.plugin), String::from(r.spec.name))));
    assert_eq!(
        readers,
        [
            ("epub".into(), "app".into()),
            ("notes".into(), "view".into())
        ]
    );

    // A plugin that stopped is not listed; its generation is no longer live.
    txt.live.set(false);
    assert_eq!(formats(&store), [(1, "epub.epub".into(), 5)]);
    store.remove_host_owner(1);
    assert!(formats(&store).is_empty());
    let mut count = 0;
    store.each_reader(&mut |_| count += 1);
    assert_eq!(count, 1);
}

#[test]
fn a_plugin_declared_again_goes_to_the_end_of_the_load_order() {
    let (a, b) = (plugin("aaa", 1), plugin("bbb", 2));
    let mut store = DeclarationStore::default();
    install(&mut store, &a, &[batch(&[format("one", "a1", 0)])]).unwrap();
    install(&mut store, &b, &[batch(&[format("two", "b1", 0)])]).unwrap();
    assert_eq!(
        formats(&store).iter().map(|f| f.0).collect::<Vec<_>>(),
        [1, 2]
    );
    // The same plugin installs a second batch after `b`.
    let (c, d) = (plugin("ccc", 3), plugin("ddd", 4));
    install(&mut store, &c, &[batch(&[format("three", "c1", 0)])]).unwrap();
    install(&mut store, &d, &[batch(&[format("four", "d1", 0)])]).unwrap();
    assert_eq!(
        formats(&store).iter().map(|f| f.0).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
}

#[test]
fn thirty_two_formats_fit_the_device_and_the_thirty_third_is_full() {
    const NAMES: [&str; 8] = ["a", "b", "c", "d", "e", "f", "g", "h"];
    let ids = ["p0", "p1", "p2", "p3", "p4"];
    let exts: Vec<String> = (0..40).map(|n| format!("x{n}")).collect();
    let mut store = DeclarationStore::default();
    let plugins: Vec<Rc<Plugin>> = ids
        .iter()
        .enumerate()
        .map(|(n, id)| plugin(id, n as u64 + 1))
        .collect();
    // Four plugins of eight formats are 32; the fifth has none to spare.
    for (n, p) in plugins.iter().take(4).enumerate() {
        let rows: Vec<Record<'_>> = NAMES
            .iter()
            .enumerate()
            .map(|(i, name)| format(name, &exts[n * 8 + i], 0))
            .collect();
        install(&mut store, p, &[batch(&rows[..4]), batch(&rows[4..])]).unwrap();
    }
    assert_eq!(formats(&store).len(), 32);
    // The 33rd is refused at the device's cap: problem 14, arg 32.
    assert_eq!(
        install_capped(&mut store, &plugins[4], &[batch(&[format("z", "zzz", 0)])]),
        (Err(ERR_FULL), Some(Cap::of(Cap::FORMATS, 32)))
    );
    // The refusal left nothing behind; a plugin that updates may keep its own.
    assert_eq!(formats(&store).len(), 32);
    store.remove_host_owner(2);
    assert_eq!(formats(&store).len(), 24);
    install(&mut store, &plugins[4], &[batch(&[format("z", "zzz", 0)])]).unwrap();
    assert_eq!(formats(&store).len(), 25);
}

#[test]
fn sixteen_reading_apps_fit_the_device_and_the_seventeenth_is_full() {
    let ids = ["p0", "p1", "p2", "p3", "p4"];
    let names = ["a", "b", "c", "d"];
    let mut store = DeclarationStore::default();
    let plugins: Vec<Rc<Plugin>> = ids
        .iter()
        .enumerate()
        .map(|(n, id)| plugin(id, n as u64 + 1))
        .collect();
    for p in plugins.iter().take(4) {
        let rows: Vec<Record<'_>> = names.iter().map(|name| reader(name)).collect();
        install(&mut store, p, &[batch(&rows)]).unwrap();
    }
    let mut count = 0;
    store.each_reader(&mut |_| count += 1);
    assert_eq!(count, 16);
    // The 17th is refused at the device's cap: problem 15, arg 16.
    assert_eq!(
        install_capped(&mut store, &plugins[4], &[batch(&[reader("z")])]),
        (Err(ERR_FULL), Some(Cap::of(Cap::READERS, 16)))
    );
    store.remove_host_owner(2);
    install(&mut store, &plugins[4], &[batch(&[reader("z")])]).unwrap();
}

#[test]
fn lookup_providers_are_listed_in_load_order_and_go_with_their_plugin() {
    let (dict, wiki) = (plugin("dict", 1), plugin("wiki", 2));
    let mut store = DeclarationStore::default();
    install(
        &mut store,
        &wiki,
        &[batch(&[lookup("wiki"), lookup("trans")])],
    )
    .unwrap();
    install(&mut store, &dict, &[batch(&[lookup("main")])]).unwrap();
    assert_eq!(
        providers(&store),
        [
            (2, "wiki.wiki".into()),
            (2, "wiki.trans".into()),
            (1, "dict.main".into())
        ]
    );
    wiki.live.set(false);
    assert_eq!(providers(&store), [(1, "dict.main".into())]);
    store.remove_host_owner(2);
    assert_eq!(providers(&store).len(), 1);
}

#[test]
fn a_plugins_providers_are_listed_in_the_order_it_declared_them() {
    // The index is in hash order: only the declaration order is the plugin's.
    let names = ["zeta", "alpha", "mid", "beta", "omega", "chi"];
    let rows: Vec<Record<'_>> = names.iter().map(|n| lookup(n)).collect();
    let mut store = DeclarationStore::default();
    install(
        &mut store,
        &plugin("dict", 1),
        &[batch(&rows[..3]), batch(&rows[3..])],
    )
    .unwrap();
    let listed: Vec<String> = providers(&store).into_iter().map(|p| p.1).collect();
    let wanted: Vec<String> = names.iter().map(|n| format!("dict.{n}")).collect();
    assert_eq!(listed, wanted);
}

#[test]
fn the_same_provider_name_in_two_plugins_is_allowed() {
    let mut store = DeclarationStore::default();
    install(&mut store, &plugin("p0", 1), &[batch(&[lookup("a")])]).unwrap();
    install(&mut store, &plugin("p1", 2), &[batch(&[lookup("a")])]).unwrap();
    assert_eq!(providers(&store), [(1, "p0.a".into()), (2, "p1.a".into())]);
}

#[test]
fn thirty_two_providers_fit_the_device_and_the_thirty_third_is_full() {
    const NAMES: [&str; 8] = ["a", "b", "c", "d", "e", "f", "g", "h"];
    let ids = ["p0", "p1", "p2", "p3", "p4"];
    let mut store = DeclarationStore::default();
    let plugins: Vec<Rc<Plugin>> = ids
        .iter()
        .enumerate()
        .map(|(n, id)| plugin(id, n as u64 + 1))
        .collect();
    for p in plugins.iter().take(4) {
        let rows: Vec<Record<'_>> = NAMES.iter().map(|n| lookup(n)).collect();
        install(&mut store, p, &[batch(&rows[..4]), batch(&rows[4..])]).unwrap();
    }
    assert_eq!(providers(&store).len(), 32);
    // Refused at the device's cap: problem 18, arg 32.
    assert_eq!(
        install_capped(&mut store, &plugins[4], &[batch(&[lookup("z")])]),
        (Err(ERR_FULL), Some(Cap::of(Cap::LOOKUPS, 32)))
    );
    assert_eq!(providers(&store).len(), 32);
    store.remove_host_owner(2);
    install(&mut store, &plugins[4], &[batch(&[lookup("z")])]).unwrap();
    assert_eq!(providers(&store).len(), 25);
}
