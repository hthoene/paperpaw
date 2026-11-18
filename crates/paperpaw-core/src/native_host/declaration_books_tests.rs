//! Book fields and badges in the declaration store: installed with their
//! plugin, listed in load order, asked for by id, gone with the plugin, sixty-four
//! fields on the device (`library.md` section 1).
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
extern crate std;
use super::super::{Allocations, DeclarationStore, ImageOwner, PendingDeclarations};
use crate::native_host::declaration_checks::Cap;
use crate::native_host::registrations::{
    BookBadgeSpec, BookFieldSpec, REGISTRATION_MAX_BYTES, Record, records_encode,
};
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

fn field<'a>(name: &'a str, flags: u32) -> Record<'a> {
    Record::BookFieldSpec(BookFieldSpec {
        name,
        title: "field.title",
        flags,
    })
}

fn badge<'a>(name: &'a str, token: u32) -> Record<'a> {
    Record::BookBadgeSpec(BookBadgeSpec {
        name,
        token,
        flags: 0,
    })
}

fn fields(store: &DeclarationStore) -> Vec<(u64, String, bool)> {
    let mut found = Vec::new();
    store.each_book_field(&mut |f| {
        found.push((
            f.owner,
            format!("{}.{}", f.plugin, f.spec.name),
            f.in_catalog(),
        ));
    });
    found
}

fn badges(store: &DeclarationStore) -> Vec<(u64, String, u32)> {
    let mut found = Vec::new();
    store.each_book_badge(&mut |b| {
        found.push((
            b.owner,
            format!("{}.{}", b.plugin, b.spec.name),
            b.spec.token,
        ));
    });
    found
}

#[test]
fn fields_and_badges_are_listed_by_plugin_id_then_name_and_go_with_their_plugin() {
    let (stars, loan, news) = (plugin("stars", 1), plugin("loan", 2), plugin("news", 3));
    let mut store = DeclarationStore::default();
    install(&mut store, &loan, &[batch(&[field("on-loan", 1)])]).unwrap();
    install(
        &mut store,
        &stars,
        &[batch(&[
            field("rating", 0),
            field("mood", 1),
            badge("new", 7),
        ])],
    )
    .unwrap();
    install(&mut store, &news, &[batch(&[badge("unread", 9)])]).unwrap();
    assert_eq!(
        fields(&store),
        [
            (2, "loan.on-loan".into(), true),
            (1, "stars.mood".into(), true),
            (1, "stars.rating".into(), false),
        ]
    );
    assert_eq!(
        badges(&store),
        [(3, "news.unread".into(), 9), (1, "stars.new".into(), 7)]
    );
    // A plugin that stopped is not listed; one removed leaves nothing.
    loan.live.set(false);
    assert_eq!(fields(&store).len(), 2);
    store.remove_host_owner(1);
    assert!(fields(&store).is_empty());
    assert_eq!(badges(&store), [(3, "news.unread".into(), 9)]);
}

#[test]
fn the_order_is_the_plugin_id_then_the_name_whatever_the_load_order() {
    // Loaded in reverse id order, declared in reverse name order.
    let (c, b, a) = (plugin("cc", 1), plugin("bb", 2), plugin("aa", 3));
    let mut store = DeclarationStore::default();
    install(
        &mut store,
        &c,
        &[batch(&[field("z", 0), field("a", 0), badge("y", 1)])],
    )
    .unwrap();
    install(&mut store, &b, &[batch(&[field("m", 0), badge("x", 2)])]).unwrap();
    install(&mut store, &a, &[batch(&[field("q", 0), badge("w", 3)])]).unwrap();
    let names: Vec<String> = fields(&store).into_iter().map(|f| f.1).collect();
    assert_eq!(names, ["aa.q", "bb.m", "cc.a", "cc.z"]);
    let names: Vec<String> = badges(&store).into_iter().map(|b| b.1).collect();
    assert_eq!(names, ["aa.w", "bb.x", "cc.y"]);
    // Byte order, not length or case: "a" < "a-b" < "ab" < "b".
    let d = plugin("dd", 4);
    install(
        &mut store,
        &d,
        &[batch(&[
            field("b", 0),
            field("ab", 0),
            field("a-b", 0),
            field("a", 0),
        ])],
    )
    .unwrap();
    let names: Vec<String> = fields(&store).into_iter().map(|f| f.1).collect();
    assert_eq!(names[4..], ["dd.a", "dd.a-b", "dd.ab", "dd.b"]);
}

#[test]
fn a_plugin_that_reloads_keeps_its_place() {
    let (a, b) = (plugin("aa", 1), plugin("bb", 2));
    let mut store = DeclarationStore::default();
    install(&mut store, &a, &[batch(&[badge("one", 1)])]).unwrap();
    install(&mut store, &b, &[batch(&[badge("two", 2)])]).unwrap();
    let before = badges(&store);
    assert_eq!(before[0].1, "aa.one");
    // `aa` is removed and loaded again, now after `bb` in load order.
    store.remove_host_owner(1);
    let again = plugin("aa", 5);
    install(&mut store, &again, &[batch(&[badge("one", 1)])]).unwrap();
    let after = badges(&store);
    assert_eq!(
        after.iter().map(|b| b.1.as_str()).collect::<Vec<_>>(),
        ["aa.one", "bb.two"]
    );
}

#[test]
fn a_field_is_found_by_its_full_id_and_says_whether_it_is_in_the_catalog() {
    let (stars, loan) = (plugin("stars", 1), plugin("loan", 2));
    let mut store = DeclarationStore::default();
    install(
        &mut store,
        &stars,
        &[batch(&[
            field("rating", 1),
            field("note", 0),
            badge("rating", 1),
        ])],
    )
    .unwrap();
    install(&mut store, &loan, &[batch(&[field("rating", 0)])]).unwrap();
    assert_eq!(store.book_field("stars.rating"), Some(true));
    assert_eq!(store.book_field("stars.note"), Some(false));
    // The same name of another plugin is another field.
    assert_eq!(store.book_field("loan.rating"), Some(false));
    // Not declared, a badge is not a field, no plugin, no dot.
    assert_eq!(store.book_field("stars.mood"), None);
    assert_eq!(store.book_field("loan.note"), None);
    assert_eq!(store.book_field("nobody.rating"), None);
    assert_eq!(store.book_field("rating"), None);
    assert_eq!(store.book_field(""), None);
    // Its plugin gone, nothing answers for it.
    stars.live.set(false);
    assert_eq!(store.book_field("stars.rating"), None);
    assert_eq!(store.book_field("loan.rating"), Some(false));
}

#[test]
fn a_field_and_a_badge_of_one_name_are_two_kinds_and_both_stand() {
    let p = plugin("stars", 1);
    let mut store = DeclarationStore::default();
    install(
        &mut store,
        &p,
        &[batch(&[field("new", 0), badge("new", 1)])],
    )
    .unwrap();
    assert_eq!(fields(&store).len(), 1);
    assert_eq!(badges(&store).len(), 1);
}

#[test]
fn sixty_four_fields_fit_the_device_and_the_sixty_fifth_is_full() {
    const NAMES: [&str; 8] = ["a", "b", "c", "d", "e", "f", "g", "h"];
    let ids = ["p0", "p1", "p2", "p3", "p4", "p5", "p6", "p7", "p8"];
    let mut store = DeclarationStore::default();
    let plugins: Vec<Rc<Plugin>> = ids
        .iter()
        .enumerate()
        .map(|(n, id)| plugin(id, n as u64 + 1))
        .collect();
    let rows: Vec<Record<'_>> = NAMES.iter().map(|name| field(name, 0)).collect();
    // Eight plugins of eight fields are 64; the ninth has none to spare.
    for p in plugins.iter().take(8) {
        install(&mut store, p, &[batch(&rows[..4]), batch(&rows[4..])]).unwrap();
    }
    assert_eq!(fields(&store).len(), 64);
    // Refused at the device's cap: problem 16, arg 64.
    assert_eq!(
        install_capped(&mut store, &plugins[8], &[batch(&[field("z", 0)])]),
        (Err(ERR_FULL), Some(Cap::of(Cap::BOOK_FIELDS, 64)))
    );
    // The refusal left nothing behind; badges are counted apart.
    assert_eq!(fields(&store).len(), 64);
    install(&mut store, &plugins[8], &[batch(&[badge("z", 1)])]).unwrap();
    // Room again once a plugin is gone.
    store.remove_host_owner(2);
    assert_eq!(fields(&store).len(), 56);
    let late = plugin("late", 10);
    install(&mut store, &late, &[batch(&[field("z", 0)])]).unwrap();
    assert_eq!(fields(&store).len(), 57);
}

#[test]
fn sixteen_badges_fit_the_device_and_the_seventeenth_is_full() {
    const NAMES: [&str; 4] = ["a", "b", "c", "d"];
    let ids = ["p0", "p1", "p2", "p3", "p4"];
    let mut store = DeclarationStore::default();
    let plugins: Vec<Rc<Plugin>> = ids
        .iter()
        .enumerate()
        .map(|(n, id)| plugin(id, n as u64 + 1))
        .collect();
    let rows: Vec<Record<'_>> = NAMES.iter().map(|name| badge(name, 1)).collect();
    // Four plugins of four badges are 16; the fifth has none to spare.
    for p in plugins.iter().take(4) {
        install(&mut store, p, &[batch(&rows)]).unwrap();
    }
    assert_eq!(badges(&store).len(), 16);
    // Refused at the device's cap: problem 17, arg 16.
    assert_eq!(
        install_capped(&mut store, &plugins[4], &[batch(&[badge("z", 1)])]),
        (Err(ERR_FULL), Some(Cap::of(Cap::BOOK_BADGES, 16)))
    );
    assert_eq!(badges(&store).len(), 16);
    store.remove_host_owner(3);
    install(&mut store, &plugins[4], &[batch(&[badge("z", 1)])]).unwrap();
    assert_eq!(badges(&store).len(), 13);
}
