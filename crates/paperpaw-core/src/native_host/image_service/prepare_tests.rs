#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! `image_prepare`: the queue, the decode and the ids.
extern crate std;
use super::*;
use crate::native_host::image_service::DEVICE_IMAGES;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NOT_FOUND};
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::image::PngWriter;

const A: u64 = 1;
/// A device with no X4 limits and a 480-pixel screen.
const ENV: Env = Env {
    screen: 480,
    colour: false,
    max_side: 4096,
    png_row_max: usize::MAX,
    turn_band: 1 << 20,
    turn_reads: usize::MAX,
};
const B: u64 = 2;

fn png(storage: &MemoryStorage, path: &str, w: u32, h: u32) {
    let mut writer = PngWriter::new(storage, path, w, h).unwrap();
    let row = std::vec![90_u8; w as usize];
    for _ in 0..h {
        writer.row(storage, path, &row).unwrap();
    }
}

fn told(t: &ImageTable) -> Option<Ready> {
    let mut got = None;
    t.take_ready(&mut |r| {
        got = Some(r.clone());
        Ok(())
    })
    .unwrap();
    got
}

fn step(t: &ImageTable, storage: &MemoryStorage) -> Option<Ready> {
    t.prepare_step(storage, ENV).unwrap();
    told(t)
}

#[test]
fn a_request_is_decoded_to_a_prepared_image_the_plugin_owns() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    png(&storage, "a.png", 40, 20);
    t.prepare(A, "a.png", 16, 7, false, &Recipe::default())
        .unwrap();
    assert!(t.prepare_pending());
    let ready = step(&t, &storage).unwrap();
    assert_eq!((ready.owner, ready.cookie, ready.status), (A, 7, READY));
    assert_ne!(ready.image, 0);
    assert!(ready.width <= 16 && ready.height <= 16 && ready.width > 0);
    // The id is the plugin's, with the size the event gave.
    let info = t.info(A, ready.image).unwrap();
    assert_eq!((info.width, info.height), (ready.width, ready.height));
    assert_eq!(t.info(B, ready.image), Err(ERR_NOT_FOUND));
    assert!(!t.prepare_pending());
    assert_eq!(step(&t, &storage), None);
    // image_release ends it and its file goes the turn after.
    let file = {
        let inner = t.lock().unwrap();
        file_name(inner.entries[0].owner, inner.entries[0].seq).unwrap()
    };
    assert!(storage.exists(&file));
    t.release(A, ready.image).unwrap();
    assert!(t.prepare_pending(), "its file is to be deleted");
    assert_eq!(step(&t, &storage), None);
    assert!(!storage.exists(&file));
    assert!(!t.prepare_pending());
}

#[test]
fn side_zero_is_the_screens_and_a_stopped_plugin_loses_everything() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    png(&storage, "a.png", 64, 64);
    t.prepare(A, "a.png", 0, 1, false, &Recipe::default())
        .unwrap();
    t.prepare_step(&storage, Env { screen: 32, ..ENV }).unwrap();
    let ready = told(&t).unwrap();
    assert_eq!((ready.status, ready.width), (READY, 32));
    t.prepare(A, "a.png", 0, 2, false, &Recipe::default())
        .unwrap();
    t.owner_stopped(A).unwrap();
    assert_eq!(step(&t, &storage), None, "its waiting request went too");
    assert_eq!(t.info(A, ready.image), Err(ERR_NOT_FOUND));
    // Its file is deleted on the next turn.
    assert_eq!(step(&t, &storage), None);
    assert!(!t.prepare_pending());
}

#[test]
fn a_bad_file_answers_with_a_status_never_an_id() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    storage.write("junk.png", b"not an image at all").unwrap();
    storage
        .write("cut.png", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR")
        .unwrap();
    for (path, status) in [
        ("junk.png", UNSUPPORTED),
        ("cut.png", DAMAGED),
        ("none.png", NOT_FOUND),
    ] {
        t.prepare(A, path, 16, 3, false, &Recipe::default())
            .unwrap();
        let ready = step(&t, &storage).unwrap();
        assert_eq!((ready.status, ready.image), (status, 0), "{path}");
    }
    // Nothing was kept: no id, no file left, no count against the plugin.
    assert!(t.lock().unwrap().entries.is_empty());
    assert_eq!(
        t.prepare(A, "", 16, 3, false, &Recipe::default()),
        Err(ERR_INVALID)
    );
}

#[test]
fn four_wait_and_eight_prepared_and_waiting_is_the_most() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    png(&storage, "a.png", 8, 8);
    for n in 0..4 {
        t.prepare(A, "a.png", 8, n, false, &Recipe::default())
            .unwrap();
    }
    assert_eq!(
        t.prepare(A, "a.png", 8, 9, false, &Recipe::default()),
        Err(ERR_FULL)
    );
    // Another plugin has its own four.
    t.prepare(B, "a.png", 8, 1, false, &Recipe::default())
        .unwrap();
    for _ in 0..4 {
        assert_eq!(step(&t, &storage).unwrap().owner, A, "oldest first");
    }
    for n in 0..4 {
        t.prepare(A, "a.png", 8, n, false, &Recipe::default())
            .unwrap();
    }
    // 4 prepared + 4 waiting is 8: no ninth.
    assert_eq!(
        t.prepare(A, "a.png", 8, 9, false, &Recipe::default()),
        Err(ERR_FULL)
    );
}

#[test]
fn ready_is_one_event_record() {
    let ready = Ready {
        owner: A,
        cookie: 5,
        status: READY,
        image: 3,
        width: 10,
        height: 20,
        luma: None,
    };
    let mut out = [0_u8; 96];
    let n = wire::record_encode_one(&ready.record(99), &mut out).unwrap();
    let (record, used) = wire::record_decode_prefix(&out[..n]).unwrap();
    assert_eq!(used, n);
    let wire::Record::EventImageReady(event) = record else {
        panic!("not image_ready");
    };
    assert_eq!(
        (
            event.now_ms,
            event.cookie,
            event.image,
            event.width,
            event.height
        ),
        (99, 5, 3, 10, 20)
    );
}

#[test]
fn boot_cleanup_removes_only_this_hosts_files_in_its_folder_a_few_at_a_time() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    for n in 0..5 {
        storage
            .write(&std::format!("{DIR}/a{n}-1f.img"), b"x")
            .unwrap();
    }
    // Not ours: another name, a file beside the folder, one further down.
    storage
        .write(&std::format!("{DIR}/notes.txt"), b"x")
        .unwrap();
    storage
        .write(&std::format!("{DIR}/a-b.img.bak"), b"x")
        .unwrap();
    storage.write(".paperpaw/cache/keep.img", b"x").unwrap();
    storage
        .write(&std::format!("{DIR}/sub/1-1.img"), b"x")
        .unwrap();
    let mut cursor = SweepCursor::default();
    assert_eq!(t.sweep_cache(&storage, 2, &mut cursor), Ok(true));
    let mut left = 0;
    storage
        .list(DIR, &mut |e| left += usize::from(ours(e.name)))
        .unwrap();
    assert_eq!(left, 3);
    let mut calls = 0;
    while t.sweep_cache(&storage, 2, &mut cursor).unwrap() {
        calls += 1;
        assert!(calls < 20, "the sweep must end");
    }
    left = 0;
    storage
        .list(DIR, &mut |e| left += usize::from(ours(e.name)))
        .unwrap();
    assert_eq!(left, 0);
    assert!(storage.exists(&std::format!("{DIR}/notes.txt")));
    assert!(storage.exists(&std::format!("{DIR}/a-b.img.bak")));
    assert!(storage.exists(".paperpaw/cache/keep.img"));
    assert!(storage.exists(&std::format!("{DIR}/sub/1-1.img")));
    // A card without the folder is fine.
    assert_eq!(
        t.sweep_cache(&MemoryStorage::new(), 2, &mut SweepCursor::default()),
        Ok(false)
    );
}

#[test]
fn boot_cleanup_spares_a_file_a_live_image_uses() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    png(&storage, "a.png", 8, 8);
    t.prepare(A, "a.png", 8, 1, false, &Recipe::default())
        .unwrap();
    let ready = step(&t, &storage).unwrap();
    assert_eq!(
        t.sweep_cache(&storage, 8, &mut SweepCursor::default()),
        Ok(false)
    );
    assert!(t.info(A, ready.image).is_ok());
    let file = {
        let inner = t.lock().unwrap();
        file_name(inner.entries[0].owner, inner.entries[0].seq).unwrap()
    };
    assert!(storage.exists(&file), "a live image keeps its file");
}

#[test]
fn prepare_decode_release_without_telling_never_grows_the_answers() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    png(&storage, "a.png", 8, 8);
    for round in 0..40 {
        t.prepare(A, "a.png", 8, round, false, &Recipe::default())
            .unwrap();
        assert!(t.prepare_step(&storage, ENV).unwrap());
        let image = t.lock().unwrap().ready[0].image;
        // The plugin lets go of it before it is told anything.
        t.release(A, image).unwrap();
        let inner = t.lock().unwrap();
        assert!(
            inner.ready.is_empty(),
            "round {round}: its answer went with it"
        );
        assert!(inner.entries.is_empty());
        assert!(inner.garbage.len() <= 1 + inner.garbage.capacity() - inner.garbage.len());
        drop(inner);
        // Its file goes the next turn.
        t.prepare_step(&storage, ENV).unwrap();
    }
    assert!(!t.prepare_pending());
}

#[test]
fn answers_not_told_count_with_the_waiting_ones_until_they_are() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    png(&storage, "a.png", 8, 8);
    for n in 0..4 {
        t.prepare(A, "a.png", 8, n, false, &Recipe::default())
            .unwrap();
    }
    while t.prepare_step(&storage, ENV).unwrap() {}
    // Four answers wait to be told: no fifth request, and no growth.
    assert_eq!(
        t.prepare(A, "a.png", 8, 9, false, &Recipe::default()),
        Err(ERR_FULL)
    );
    assert_eq!(t.prepare_room(A), 0);
    assert_eq!(t.lock().unwrap().ready.len(), 4);
    assert!(told(&t).is_some());
    assert_eq!(t.prepare_room(A), 1);
    t.prepare(A, "a.png", 8, 9, false, &Recipe::default())
        .unwrap();
}

#[test]
fn every_prepared_image_of_every_plugin_can_end_at_once_without_losing_a_file() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    png(&storage, "a.png", 8, 8);
    let owners = [A, B, 3, 4];
    // Four plugins with four images each: more than the eight a plugin has.
    let mut images = std::vec::Vec::new();
    for owner in owners {
        for n in 0..4 {
            t.prepare(owner, "a.png", 8, n, false, &Recipe::default())
                .unwrap();
        }
    }
    while t.prepare_step(&storage, ENV).unwrap() {}
    for owner in owners {
        for _ in 0..4 {
            let ready = {
                let mut got = None;
                t.take_ready(&mut |r| {
                    if r.owner == owner {
                        got = Some(r.clone());
                    }
                    Ok(())
                })
                .unwrap();
                got
            };
            let _ = ready;
        }
    }
    {
        let inner = t.lock().unwrap();
        images.extend(
            inner
                .entries
                .iter()
                .map(|e| (e.owner, e.id, file_name(e.owner, e.seq).unwrap())),
        );
    }
    assert_eq!(images.len(), 16);
    // One more prepared image, with the table at its fullest: its room too.
    let (last_owner, last_id, _) = images[15].clone();
    for (owner, id, _) in &images {
        t.release(*owner, *id).unwrap();
    }
    let _ = (last_owner, last_id);
    let listed = t.lock().unwrap().garbage.len();
    assert_eq!(listed, 16, "every ended image's file is listed, none lost");
    for (_, _, path) in &images {
        assert!(storage.exists(path));
    }
    while t.prepare_pending() {
        t.prepare_step(&storage, ENV).unwrap();
    }
    for (_, _, path) in &images {
        assert!(!storage.exists(path), "{path}");
    }
}

#[test]
fn the_eighth_prepared_image_then_a_ninth_plugins_first_has_room_for_its_file() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    png(&storage, "a.png", 8, 8);
    for round in 0..2 {
        for n in 0..4 {
            t.prepare(A, "a.png", 8, round * 4 + n, false, &Recipe::default())
                .unwrap();
        }
        while t.prepare_step(&storage, ENV).unwrap() {}
        for _ in 0..4 {
            told(&t).unwrap();
        }
    }
    assert_eq!(t.lock().unwrap().entries.len(), 8);
    t.prepare(B, "a.png", 8, 1, false, &Recipe::default())
        .unwrap();
    assert!(t.prepare_step(&storage, ENV).unwrap());
    // All nine can end: the list has a place for each file.
    let inner = t.lock().unwrap();
    assert!(inner.garbage.capacity() >= inner.garbage.len() + inner.entries.len());
}

#[test]
fn files_a_sweep_must_leave_do_not_hide_the_deletable_ones_behind_them() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    png(&storage, "a.png", 8, 8);
    // Three live images' files sort first; four stale ones come after.
    for n in 0..3 {
        t.prepare(A, "a.png", 8, n, false, &Recipe::default())
            .unwrap();
    }
    while t.prepare_step(&storage, ENV).unwrap() {}
    let live: std::vec::Vec<String> = t
        .lock()
        .unwrap()
        .entries
        .iter()
        .map(|e| file_name(e.owner, e.seq).unwrap())
        .collect();
    assert_eq!(live.len(), 3);
    for n in 0..4 {
        storage
            .write(&std::format!("{DIR}/ff{n}-1.img"), b"x")
            .unwrap();
    }
    // A window of two is filled by live files at first, yet it ends.
    let mut cursor = SweepCursor::default();
    let mut calls = 0;
    while t.sweep_cache(&storage, 2, &mut cursor).unwrap() {
        calls += 1;
        assert!(calls < 20, "the sweep must end");
    }
    for n in 0..4 {
        assert!(!storage.exists(&std::format!("{DIR}/ff{n}-1.img")));
    }
    for path in &live {
        assert!(storage.exists(path), "a live image's file stays");
    }
}

#[test]
fn the_x4_clamps_the_side_and_refuses_a_wide_png_source_row() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    png(&storage, "big.png", 64, 64);
    t.prepare(A, "big.png", 0, 1, false, &Recipe::default())
        .unwrap();
    let x4 = Env {
        screen: 480,
        colour: false,
        max_side: 16,
        png_row_max: 8192,
        turn_band: 32 * 1024,
        turn_reads: 16_384,
    };
    assert!(t.prepare_step(&storage, x4).unwrap());
    let ready = told(&t).unwrap();
    assert_eq!(ready.status, READY);
    assert!(
        ready.width <= 16 && ready.height <= 16,
        "side 0 and 4096 stop at 16"
    );
    t.prepare(A, "big.png", 4096, 2, false, &Recipe::default())
        .unwrap();
    t.prepare_step(&storage, x4).unwrap();
    let ready = told(&t).unwrap();
    assert!(ready.width <= 16, "a larger side is clamped too");
    // A source row of 64 pixels of 8-bit gray is 64 bytes: over a cap of 63.
    let narrow = Env {
        png_row_max: 63,
        ..ENV
    };
    t.prepare(A, "big.png", 8, 3, false, &Recipe::default())
        .unwrap();
    t.prepare_step(&storage, narrow).unwrap();
    let ready = told(&t).unwrap();
    assert_eq!((ready.status, ready.image), (TOO_LARGE, 0));
    let ok = Env {
        png_row_max: 64,
        ..ENV
    };
    t.prepare(A, "big.png", 8, 4, false, &Recipe::default())
        .unwrap();
    t.prepare_step(&storage, ok).unwrap();
    assert_eq!(told(&t).unwrap().status, READY);
}

#[test]
fn a_cache_name_left_by_an_earlier_boot_is_replaced_and_a_sweep_spares_the_live_file() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    png(&storage, "a.png", 8, 8);
    // The first number a fresh table gives is the one an earlier boot left.
    let stale = file_name(A, 0).unwrap();
    storage.write(&stale, b"junk from the last boot").unwrap();
    t.prepare(A, "a.png", 8, 1, false, &Recipe::default())
        .unwrap();
    let ready = step(&t, &storage).unwrap();
    assert_eq!(ready.status, READY, "the old file is replaced, not trusted");
    assert!(paperpaw_sdk::ui::probe_picture(&storage, &stale).is_ok());
    let mut cursor = SweepCursor::default();
    assert_eq!(t.sweep_cache(&storage, 8, &mut cursor), Ok(false));
    assert!(storage.exists(&stale), "a live image's file stays");
    assert!(t.info(A, ready.image).is_ok());
}

#[test]
fn the_device_holds_sixty_four_ids_at_most() {
    let t = ImageTable::new();
    let owners: std::vec::Vec<u64> = (1..=9).collect();
    let mut made = 0;
    'fill: for owner in &owners {
        for n in 0..PREPARED_IMAGES as u32 {
            match t.add_prepared(*owner, n, 8, 8, false) {
                Ok(_) => made += 1,
                Err(error) => {
                    assert_eq!(error, ERR_FULL);
                    break 'fill;
                }
            }
        }
    }
    assert_eq!(made, DEVICE_IMAGES);
    // A document image of a plugin under its own cap waits, too.
    assert_eq!(
        t.add(
            10,
            Origin::Document {
                document: 1,
                index: 0
            },
            "c/x",
            8,
            8,
            false
        ),
        Err(paperpaw_native::host_glue::ERR_BUSY)
    );
}

/// A card that will not delete one name.
struct Stubborn {
    inner: MemoryStorage,
    name: std::string::String,
}

impl Storage for Stubborn {
    fn read_at(
        &self,
        p: &str,
        o: u64,
        b: &mut [u8],
    ) -> Result<usize, paperpaw_platform::StorageError> {
        self.inner.read_at(p, o, b)
    }
    fn size(&self, p: &str) -> Result<u64, paperpaw_platform::StorageError> {
        self.inner.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.inner.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.inner.append(p, d)
    }
    fn list(
        &self,
        d: &str,
        v: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), paperpaw_platform::StorageError> {
        self.inner.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), paperpaw_platform::StorageError> {
        if p.ends_with(&self.name) {
            return Err(paperpaw_platform::StorageError::Io);
        }
        self.inner.remove(p)
    }
}

#[test]
fn a_sweep_reaches_dead_files_behind_many_foreign_and_undeletable_ones() {
    let t = ImageTable::new();
    let storage = Stubborn {
        inner: MemoryStorage::new(),
        name: "a0-1.img".into(),
    };
    // More foreign entries than one call looks at, before and between ours.
    for n in 0..(SWEEP_SCAN * 3) {
        storage
            .write(&std::format!("{DIR}/note{n:03}.txt"), b"x")
            .unwrap();
    }
    for n in 0..6 {
        storage
            .write(&std::format!("{DIR}/a{n}-1.img"), b"x")
            .unwrap();
    }
    let mut cursor = SweepCursor::default();
    let mut calls = 0;
    while t.sweep_cache(&storage, 2, &mut cursor).unwrap() {
        calls += 1;
        assert!(calls < 100, "the sweep must end");
    }
    for n in 1..6 {
        assert!(!storage.exists(&std::format!("{DIR}/a{n}-1.img")), "a{n}");
    }
    assert!(
        storage.exists(&std::format!("{DIR}/a0-1.img")),
        "undeletable stays"
    );
    assert!(storage.exists(&std::format!("{DIR}/note000.txt")));
}

#[test]
fn a_sweep_with_no_budget_is_refused_and_changes_nothing() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    storage
        .write(&std::format!("{DIR}/a0-1.img"), b"x")
        .unwrap();
    let mut cursor = SweepCursor::default();
    assert_eq!(t.sweep_cache(&storage, 0, &mut cursor), Err(ERR_INVALID));
    assert_eq!(cursor, SweepCursor::default());
    assert!(storage.exists(&std::format!("{DIR}/a0-1.img")));
}

#[test]
fn the_x4_environment_sets_its_limits() {
    let env = Env::x4(480, true);
    assert_eq!((env.max_side, env.png_row_max), (1_600, 8 * 1024));
    assert_eq!((env.screen, env.colour), (480, true));
}

/// A card that lists a folder from a place without reading what is before
/// it, and says how many entries it handed out.
struct Paged {
    inner: MemoryStorage,
    visited: core::cell::Cell<usize>,
}

impl Storage for Paged {
    fn read_at(
        &self,
        p: &str,
        o: u64,
        b: &mut [u8],
    ) -> Result<usize, paperpaw_platform::StorageError> {
        self.inner.read_at(p, o, b)
    }
    fn size(&self, p: &str) -> Result<u64, paperpaw_platform::StorageError> {
        self.inner.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.inner.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.inner.append(p, d)
    }
    fn list(
        &self,
        _: &str,
        _: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), paperpaw_platform::StorageError> {
        panic!("a sweep never lists a whole folder");
    }
    fn list_from(
        &self,
        _: &str,
        _: u64,
        _: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, paperpaw_platform::StorageError> {
        panic!("a sweep reads its folder with list_bounded");
    }
    fn list_bounded(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, paperpaw_platform::StorageError> {
        let mut names = std::vec::Vec::new();
        self.inner.list(dir, &mut |e| {
            names.push((std::string::String::from(e.name), e.is_dir, e.size));
        })?;
        // A place that no longer holds starts the listing over.
        let from = if from as usize > names.len() {
            0
        } else {
            from as usize
        };
        for (at, (name, is_dir, size)) in names.iter().enumerate().skip(from) {
            self.visited.set(self.visited.get() + 1);
            if !visit(paperpaw_platform::DirEntry::new(name, *is_dir, *size)) {
                return Ok(Some(at as u64));
            }
        }
        Ok(None)
    }
    fn remove(&self, p: &str) -> Result<(), paperpaw_platform::StorageError> {
        self.inner.remove(p)
    }
}

#[test]
fn a_sweep_call_reads_at_most_a_bounded_piece_from_the_start_a_place_or_a_stale_place() {
    let t = ImageTable::new();
    let card = Paged {
        inner: MemoryStorage::new(),
        visited: core::cell::Cell::new(0),
    };
    for n in 0..200 {
        card.write(&std::format!("{DIR}/n{n:03}.txt"), b"x")
            .unwrap();
    }
    for n in 0..5 {
        card.write(&std::format!("{DIR}/a{n}-1.img"), b"x").unwrap();
    }
    let mut calls = 0;
    // From the start, then resumed, then (a cursor the card no longer has)
    // stale: each call hands out at most the entries looked at, and one more
    // (the one it stopped at).
    for stale in [false, true] {
        let mut cursor = SweepCursor::default();
        if stale {
            card.write(&std::format!("{DIR}/a0-1.img"), b"x").unwrap();
            cursor = SweepCursor::default();
            // Walk a call in, then make the folder shorter than the place.
            assert_eq!(t.sweep_cache(&card, 1, &mut cursor), Ok(true));
            for n in 0..200 {
                card.remove(&std::format!("{DIR}/n{n:03}.txt")).unwrap();
            }
        }
        loop {
            card.visited.set(0);
            let more = t.sweep_cache(&card, 1, &mut cursor).unwrap();
            assert!(card.visited.get() <= SWEEP_SCAN + 1, "a bounded piece");
            calls += 1;
            assert!(calls < 400, "the sweep must end");
            if !more {
                break;
            }
        }
    }
    for n in 0..5 {
        assert!(!card.exists(&std::format!("{DIR}/a{n}-1.img")), "a{n}");
    }
    assert!(calls > 8, "{calls}: more than one call was needed");
}

/// A PNG header only: signature, `IHDR` with the given size.
fn png_header(width: u32, height: u32) -> std::vec::Vec<u8> {
    let mut bytes = std::vec::Vec::from(*b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR");
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&[8, 0, 0, 0, 0]);
    bytes
}

#[test]
fn a_source_over_eight_megapixels_is_refused_from_its_header_before_decoding() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    // Headers only: a decoder that started would find damage, not size.
    storage
        .write("huge.png", &png_header(4_000, 2_001))
        .unwrap();
    t.prepare(A, "huge.png", 8, 1, false, &Recipe::default())
        .unwrap();
    let ready = step(&t, &storage).unwrap();
    assert_eq!((ready.status, ready.image), (TOO_LARGE, 0));
    // A GIF the same way.
    storage
        .write("huge.gif", &gif_with_frame((5_000, 5_000), (5_000, 5_000)))
        .unwrap();
    t.prepare(A, "huge.gif", 8, 2, false, &Recipe::default())
        .unwrap();
    assert_eq!(step(&t, &storage).unwrap().status, TOO_LARGE);
    // A JPEG whose frame header comes after a long application segment.
    let mut jpeg = std::vec![0xff, 0xd8, 0xff, 0xe1, 0x10, 0x00];
    jpeg.extend_from_slice(&[0; 0x0ffe]);
    jpeg.extend_from_slice(&[0xff, 0xc0, 0, 17, 8]);
    jpeg.extend_from_slice(&3_000_u16.to_be_bytes());
    jpeg.extend_from_slice(&3_000_u16.to_be_bytes());
    storage.write("huge.jpg", &jpeg).unwrap();
    t.prepare(A, "huge.jpg", 8, 3, false, &Recipe::default())
        .unwrap();
    assert_eq!(step(&t, &storage).unwrap().status, TOO_LARGE);
    // Exactly 8 Mpx passes the check (the damaged body then fails the decode).
    assert_eq!(source_pixels(&storage, "huge.jpg"), Ok(Some(9_000_000)));
    storage
        .write("edge.png", &png_header(4_000, 2_000))
        .unwrap();
    assert_eq!(
        source_pixels(&storage, "edge.png"),
        Ok(Some(SOURCE_PIXELS_MAX))
    );
    t.prepare(A, "edge.png", 8, 4, false, &Recipe::default())
        .unwrap();
    assert_ne!(step(&t, &storage).unwrap().status, TOO_LARGE);
    // A header with no size, or a JPEG with no frame header, is damaged.
    storage.write("zero.png", &png_header(0, 5)).unwrap();
    assert_eq!(source_pixels(&storage, "zero.png"), Err(DAMAGED));
    storage
        .write("cut.jpg", &[0xff, 0xd8, 0xff, 0xda, 0, 2])
        .unwrap();
    assert_eq!(source_pixels(&storage, "cut.jpg"), Err(DAMAGED));
    storage.write("short.png", &png_header(4, 4)[..20]).unwrap();
    assert_eq!(source_pixels(&storage, "short.png"), Err(DAMAGED));
    // A format this does not read is the decoder's to refuse.
    storage.write("x.bin", b"plain text here").unwrap();
    assert_eq!(source_pixels(&storage, "x.bin"), Ok(None));
}

/// A GIF of a `screen` with a first frame of `frame`, after a comment
/// extension of two sub-blocks.
fn gif_with_frame(screen: (u16, u16), frame: (u16, u16)) -> std::vec::Vec<u8> {
    let mut bytes = std::vec::Vec::from(*b"GIF89a");
    bytes.extend_from_slice(&screen.0.to_le_bytes());
    bytes.extend_from_slice(&screen.1.to_le_bytes());
    // A global colour table of 2 entries, background, aspect.
    bytes.extend_from_slice(&[0x80, 0, 0, 0, 0, 0, 255, 255, 255]);
    bytes.extend_from_slice(&[0x21, 0xfe, 3, b'a', b'b', b'c', 2, b'd', b'e', 0]);
    bytes.extend_from_slice(&[0x2c, 0, 0, 0, 0]);
    bytes.extend_from_slice(&frame.0.to_le_bytes());
    bytes.extend_from_slice(&frame.1.to_le_bytes());
    bytes.push(0);
    bytes
}

#[test]
fn a_gif_frame_is_counted_and_may_not_be_larger_than_its_screen() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    // A tiny logical screen with a huge frame: the decoder would make the
    // frame, so the file is refused before it is decoded.
    storage
        .write("bypass.gif", &gif_with_frame((4, 4), (5_000, 5_000)))
        .unwrap();
    assert_eq!(source_pixels(&storage, "bypass.gif"), Err(DAMAGED));
    t.prepare(A, "bypass.gif", 8, 1, false, &Recipe::default())
        .unwrap();
    let ready = step(&t, &storage).unwrap();
    assert_eq!((ready.status, ready.image), (DAMAGED, 0));
    // A frame inside its screen is counted at the larger of the two.
    storage
        .write("ok.gif", &gif_with_frame((100, 50), (60, 40)))
        .unwrap();
    assert_eq!(source_pixels(&storage, "ok.gif"), Ok(Some(5_000)));
    // Frame and screen both over 8 Mpx: too large.
    storage
        .write("huge.gif", &gif_with_frame((4_000, 2_001), (4_000, 2_001)))
        .unwrap();
    assert_eq!(source_pixels(&storage, "huge.gif"), Ok(Some(8_004_000)));
    t.prepare(A, "huge.gif", 8, 2, false, &Recipe::default())
        .unwrap();
    assert_eq!(step(&t, &storage).unwrap().status, TOO_LARGE);
    // No image descriptor (the file ends in the extension), or one with no
    // size, is damaged.
    let mut cut = gif_with_frame((4, 4), (2, 2));
    cut.truncate(cut.len() - 10);
    storage.write("cut.gif", &cut).unwrap();
    assert_eq!(source_pixels(&storage, "cut.gif"), Err(DAMAGED));
    storage
        .write("zero.gif", &gif_with_frame((4, 4), (0, 2)))
        .unwrap();
    assert_eq!(source_pixels(&storage, "zero.gif"), Err(DAMAGED));
}

#[test]
fn the_longest_side_is_1600_everywhere() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    png(&storage, "p.png", 40, 20);
    // A device-wide env that would allow 4,096 still stops at the contract.
    let roomy = Env {
        max_side: 4096,
        ..ENV
    };
    assert_eq!(crate::native_host::image_service::MAX_SIDE, 1_600);
    t.prepare(A, "p.png", 4096, 1, false, &Recipe::default())
        .unwrap();
    t.prepare_step(&storage, roomy).unwrap();
    assert_eq!(told(&t).unwrap().status, READY);
    assert_eq!(
        t.add_prepared(A, 9, 1_601, 4, false),
        Err(ERR_INVALID),
        "a prepared image over 1,600 is never kept"
    );
}
