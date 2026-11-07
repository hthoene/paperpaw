#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! `image_prepare`: the queue, the decode and the ids.
extern crate std;
use super::*;
use paperpaw_native::host_glue::ERR_NOT_FOUND;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::image::PngWriter;

const A: u64 = 1;
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
    t.prepare_step(storage, 480, false).unwrap();
    told(t)
}

#[test]
fn a_request_is_decoded_to_a_prepared_image_the_plugin_owns() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    png(&storage, "a.png", 40, 20);
    t.prepare(A, "a.png", 16, 7, false).unwrap();
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
        inner.entries[0].path.clone()
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
    t.prepare(A, "a.png", 0, 1, false).unwrap();
    t.prepare_step(&storage, 32, false).unwrap();
    let ready = told(&t).unwrap();
    assert_eq!((ready.status, ready.width), (READY, 32));
    t.prepare(A, "a.png", 0, 2, false).unwrap();
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
        ("none.png", CARD),
    ] {
        t.prepare(A, path, 16, 3, false).unwrap();
        let ready = step(&t, &storage).unwrap();
        assert_eq!((ready.status, ready.image), (status, 0), "{path}");
    }
    // Nothing was kept: no id, no file left, no count against the plugin.
    assert!(t.lock().unwrap().entries.is_empty());
    assert_eq!(t.prepare(A, "", 16, 3, false), Err(ERR_INVALID));
}

#[test]
fn four_wait_and_eight_prepared_and_waiting_is_the_most() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    png(&storage, "a.png", 8, 8);
    for n in 0..4 {
        t.prepare(A, "a.png", 8, n, false).unwrap();
    }
    assert_eq!(t.prepare(A, "a.png", 8, 9, false), Err(ERR_FULL));
    // Another plugin has its own four.
    t.prepare(B, "a.png", 8, 1, false).unwrap();
    for _ in 0..4 {
        assert_eq!(step(&t, &storage).unwrap().owner, A, "oldest first");
    }
    for n in 0..4 {
        t.prepare(A, "a.png", 8, n, false).unwrap();
    }
    // 4 prepared + 4 waiting is 8: no ninth.
    assert_eq!(t.prepare(A, "a.png", 8, 9, false), Err(ERR_FULL));
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
    };
    let mut out = [0_u8; 64];
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
    assert_eq!(t.sweep_cache(&storage, 2), Ok(true));
    let mut left = 0;
    storage
        .list(DIR, &mut |e| left += usize::from(ours(e.name)))
        .unwrap();
    assert_eq!(left, 3);
    assert_eq!(t.sweep_cache(&storage, 2), Ok(true));
    assert_eq!(
        t.sweep_cache(&storage, 2),
        Ok(false),
        "the last one: nothing more"
    );
    assert!(storage.exists(&std::format!("{DIR}/notes.txt")));
    assert!(storage.exists(&std::format!("{DIR}/a-b.img.bak")));
    assert!(storage.exists(".paperpaw/cache/keep.img"));
    assert!(storage.exists(&std::format!("{DIR}/sub/1-1.img")));
    // A card without the folder is fine.
    assert_eq!(t.sweep_cache(&MemoryStorage::new(), 2), Ok(false));
}

#[test]
fn boot_cleanup_spares_a_file_a_live_image_uses() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    png(&storage, "a.png", 8, 8);
    t.prepare(A, "a.png", 8, 1, false).unwrap();
    let ready = step(&t, &storage).unwrap();
    assert_eq!(t.sweep_cache(&storage, 8), Ok(false));
    assert!(t.info(A, ready.image).is_ok());
    let file = t.lock().unwrap().entries[0].path.clone();
    assert!(storage.exists(&file), "a live image keeps its file");
}
