//! The index kept in line with the declaration store: a format declared is
//! listed once and keeps its reference, one that goes is gone for good, a
//! changed one is another, readers follow their plugin, a refused allocation
//! changes nothing and is tried again.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
extern crate std;
use super::*;
use crate::native_host::{
    declaration_store::{Allocations, DeclarationStore, ImageOwner, PendingDeclarations},
    registrations::{REGISTRATION_MAX_BYTES, Record, records_encode},
};
use alloc::{rc::Rc, vec, vec::Vec};
use core::{cell::Cell, ops::ControlFlow};
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
    Record::FormatSpec(wire::FormatSpec {
        name,
        title: "Title",
        extensions,
        flags: 0,
        priority,
        token: 3,
    })
}

fn reader<'a>(name: &'a str, formats: &'a str) -> Record<'a> {
    Record::ReaderSpec(wire::ReaderSpec {
        name,
        screen: "x.main",
        formats,
        flags: 0,
    })
}

fn install(store: &mut DeclarationStore, plugin: &Rc<Plugin>, records: &[Record<'_>]) {
    let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
    let len = records_encode(records, &mut bytes).unwrap();
    bytes.truncate(len);
    let mut allocations = Allocations::default();
    let mut pending = PendingDeclarations::new(Id::try_parse(plugin.id).unwrap(), plugin.clone());
    pending.stage_batch(&bytes, &mut allocations).unwrap();
    store.prepare(pending, &mut allocations).unwrap().commit();
}

fn ids(index: &FormatIndex) -> Vec<alloc::string::String> {
    let mut found = Vec::new();
    index
        .formats(0, 0, &mut |row| {
            if let Record::ServiceDocFormat(f) = row {
                found.push(f.id.into());
            }
            ControlFlow::Continue(())
        })
        .unwrap();
    found
}

fn gone_of(index: &mut FormatIndex, store: &DeclarationStore) -> Vec<FormatRef> {
    let mut gone = Vec::new();
    index.sync(store, |reference| gone.push(reference)).unwrap();
    gone
}

#[test]
fn a_declared_format_is_listed_once_with_a_reference_that_stays() {
    let (epub, txt) = (plugin("epub", 1), plugin("txt", 2));
    let mut store = DeclarationStore::default();
    let mut index = FormatIndex::new();
    assert!(gone_of(&mut index, &store).is_empty());
    assert!(ids(&index).is_empty());
    install(&mut store, &epub, &[format("epub", "epub", 1)]);
    assert!(gone_of(&mut index, &store).is_empty());
    let first = index.by_id("epub.epub").unwrap();
    let generation = index.generation.get();
    // Again: nothing moves.
    assert!(gone_of(&mut index, &store).is_empty());
    assert_eq!(index.generation.get(), generation);
    install(&mut store, &txt, &[format("txt", "txt", 0)]);
    assert!(gone_of(&mut index, &store).is_empty());
    assert_eq!(ids(&index), ["epub.epub", "txt.txt"]);
    assert_eq!(index.by_id("epub.epub"), Some(first));
    assert_ne!(index.generation.get(), generation);
}

#[test]
fn a_stopped_plugins_formats_are_gone_and_a_reference_is_never_reused() {
    let (epub, txt) = (plugin("epub", 1), plugin("txt", 2));
    let mut store = DeclarationStore::default();
    let mut index = FormatIndex::new();
    install(&mut store, &epub, &[format("epub", "epub", 1)]);
    install(&mut store, &txt, &[format("txt", "txt", 0)]);
    gone_of(&mut index, &store);
    let old = index.by_id("epub.epub").unwrap();
    epub.live.set(false);
    assert_eq!(gone_of(&mut index, &store), [old]);
    assert_eq!(ids(&index), ["txt.txt"]);
    assert_eq!(index.target(old), None);
    // The same format declared again by a new plugin instance (installed
    // anew: the old one is removed first) is another one.
    store.remove_host_owner(1);
    let again = plugin("epub", 3);
    install(&mut store, &again, &[format("epub", "epub", 1)]);
    assert!(gone_of(&mut index, &store).is_empty());
    let new = index.by_id("epub.epub").unwrap();
    assert_ne!(new, old);
    assert_eq!(index.target(old), None);
    assert_eq!(index.target(new).map(|t| t.plugin), Some(3));
}

#[test]
fn a_format_declared_differently_is_another_format_and_its_documents_end() {
    let epub = plugin("epub", 1);
    let mut store = DeclarationStore::default();
    let mut index = FormatIndex::new();
    install(&mut store, &epub, &[format("epub", "epub", 1)]);
    gone_of(&mut index, &store);
    let old = index.by_id("epub.epub").unwrap();
    // The same owner, a higher priority: the store holds the new record only.
    store.remove_host_owner(1);
    let epub = plugin("epub", 1);
    install(&mut store, &epub, &[format("epub", "epub", 9)]);
    assert_eq!(gone_of(&mut index, &store), [old]);
    let new = index.by_id("epub.epub").unwrap();
    assert_ne!(new, old);
}

#[test]
fn readers_follow_their_plugin_and_are_replaced_whole() {
    let (epub, notes) = (plugin("epub", 1), plugin("notes", 2));
    let mut store = DeclarationStore::default();
    let mut index = FormatIndex::new();
    install(&mut store, &epub, &[reader("app", "")]);
    gone_of(&mut index, &store);
    let rows = |index: &FormatIndex, scope: &str| {
        let mut found = Vec::new();
        index
            .readers(scope, 0, 0, &mut |row| {
                if let Record::ServiceDocReader(r) = row {
                    found.push((
                        alloc::string::String::from(r.id),
                        alloc::string::String::from(r.title),
                    ));
                }
                ControlFlow::Continue(())
            })
            .unwrap();
        found
    };
    assert_eq!(rows(&index, ""), [("epub.app".into(), "epub".into())]);
    let generation = index.generation.get();
    gone_of(&mut index, &store);
    assert_eq!(
        index.generation.get(),
        generation,
        "unchanged: no new generation"
    );
    install(&mut store, &notes, &[reader("view", "epub.epub")]);
    gone_of(&mut index, &store);
    assert_eq!(rows(&index, "").len(), 2);
    assert_eq!(rows(&index, "epub.epub").len(), 2);
    assert_eq!(
        rows(&index, "txt.txt").len(),
        1,
        "an app listing none reads all"
    );
    epub.live.set(false);
    gone_of(&mut index, &store);
    assert_eq!(rows(&index, ""), [("notes.view".into(), "notes".into())]);
}
