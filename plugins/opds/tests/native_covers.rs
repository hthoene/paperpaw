//! The covers of a feed without a host: which entry is next, one download at
//! a time and a few pictures held, an answer for an older feed dropped, the
//! cache's names and its index (the oldest files go).
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
mod fakes;
use fakes::Mem;
use paperpaw_plugin_opds::native_covers::{
    self as covers, Covers, MAX_CACHED, MAX_LIVE, cache_name, cached, remember,
};
use paperpaw_sdk::native::{image::ImageId, net::RequestId};

fn image(n: u32) -> ImageId {
    ImageId::new(n).unwrap()
}

fn request(n: u32) -> RequestId {
    RequestId::new(n).unwrap()
}

#[test]
fn the_first_visible_entry_that_wants_a_cover_is_next() {
    let covers = Covers::new(1, 10).unwrap();
    let wants = |i: usize| i % 2 == 1;
    assert_eq!(covers.pick([0, 1, 2, 3], wants), Some(1));
    // Only the entries on the page shown.
    assert_eq!(covers.pick([4, 6, 8], wants), None);
    assert_eq!(covers.pick([], wants), None);
    // An entry past the feed is nothing.
    assert_eq!(covers.pick([99], |_| true), None);
}

#[test]
fn one_cover_is_downloaded_at_a_time_and_each_is_tried_once() {
    let mut covers = Covers::new(1, 4).unwrap();
    covers.fetching(0, request(7));
    assert_eq!(covers.pick([1, 2], |_| true), None, "one at a time");
    // The wrong request ends nothing.
    assert_eq!(covers.fetched(request(8)), None);
    assert_eq!(covers.fetched(request(7)), Some(0));
    assert_eq!(
        covers.pick([0, 1], |_| true),
        Some(1),
        "0 is not Idle any more"
    );
    covers.failed(1);
    assert_eq!(
        covers.pick([0, 1], |_| true),
        None,
        "a failed one is not tried again"
    );
}

#[test]
fn at_most_a_few_pictures_are_held() {
    let n = MAX_LIVE + 3;
    let mut covers = Covers::new(1, n).unwrap();
    for i in 0..MAX_LIVE {
        covers.preparing(i);
    }
    assert_eq!(covers.pick(0..n, |_| true), None);
    // One is given back (the feed is left): the others may be had.
    let cookie = covers.cookie(0);
    assert_eq!(covers.ready(cookie, Some((image(1), 40, 60))), None);
    assert_eq!(covers.image(0), Some((image(1), 40, 60)));
    let ids = covers.release_all().unwrap();
    assert_eq!(ids, [image(1)]);
    assert_eq!(covers.image(0), None);
}

#[test]
fn a_picture_of_a_feed_shown_before_is_handed_back_to_be_released() {
    let mut old = Covers::new(1, 3).unwrap();
    old.preparing(2);
    let cookie = old.cookie(2);
    // The feed changed: its covers are new; the old cookie means nothing.
    let mut current = Covers::new(2, 3).unwrap();
    current.preparing(2);
    assert_eq!(
        current.ready(cookie, Some((image(5), 10, 10))),
        Some(image(5))
    );
    assert_eq!(current.image(2), None);
    // An entry that is gone, and a picture nobody waits for.
    let mut few = Covers::new(1, 1).unwrap();
    assert_eq!(
        few.ready(few.cookie(9), Some((image(6), 1, 1))),
        Some(image(6))
    );
    assert_eq!(
        few.ready(few.cookie(0), Some((image(7), 1, 1))),
        Some(image(7))
    );
    // A picture that did not come marks the entry failed.
    few.preparing(0);
    assert_eq!(few.ready(few.cookie(0), None), None);
    assert_eq!(few.pick([0], |_| true), None);
}

#[test]
fn a_cover_is_cached_by_the_digest_of_its_address() {
    let name = cache_name("https://a.example/c.jpg").unwrap();
    assert!(
        name.starts_with("covers/") && name.ends_with(".img"),
        "{name}"
    );
    assert_eq!(name.len(), "covers/".len() + 32 + ".img".len());
    assert_ne!(name, cache_name("https://a.example/d.jpg").unwrap());
    let mem = Mem::default();
    assert!(!cached(&mem, &name));
    mem.put(&name, b"x");
    assert!(cached(&mem, &name));
    mem.put(&name, b"");
    assert!(!cached(&mem, &name), "an empty file is no cover");
}

#[test]
fn the_oldest_cover_files_go_past_the_cache_size() {
    let mem = Mem::default();
    let names: Vec<String> = (0..MAX_CACHED + 5)
        .map(|n| cache_name(&format!("https://a.example/{n}.jpg")).unwrap())
        .collect();
    for name in &names {
        mem.put(name, b"jpeg");
        remember(&mem, name).unwrap();
    }
    for name in &names[..5] {
        assert!(!mem.has(name), "{name} went");
    }
    for name in &names[5..] {
        assert!(mem.has(name), "{name} stays");
    }
    assert_eq!(covers::index(&mem).unwrap().lines().count(), MAX_CACHED);
    // Noted again, a file is the newest and not twice in the index.
    remember(&mem, &names[5]).unwrap();
    let index = covers::index(&mem).unwrap();
    assert_eq!(index.lines().count(), MAX_CACHED);
    assert_eq!(index.lines().last(), Some(names[5].as_str()));
}

#[test]
fn an_index_that_cannot_be_read_is_begun_anew() {
    let mem = Mem::default();
    mem.put("covers.txt", b"\xff\xfe not text");
    let name = cache_name("https://a.example/x.jpg").unwrap();
    remember(&mem, &name).unwrap();
    assert_eq!(covers::index(&mem).unwrap(), format!("{name}\n"));
    // A card that is full leaves the cover as it is: no error.
    mem.full_after.set(Some(mem.writes.get()));
    remember(&mem, &name).unwrap();
}
