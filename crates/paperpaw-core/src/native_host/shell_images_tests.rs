#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
extern crate std;
use super::*;
use paperpaw_native::host_glue::ERR_BUSY;
use paperpaw_sdk::MemoryStorage;

#[test]
fn roots_enforce_owner_and_storage_authority() {
    let plugin = Id::try_parse("test").unwrap();
    let roots = OwnerRoots::new(19, &plugin, false);
    assert_eq!(
        roots.resolve(19, 0, "a.png").unwrap(),
        ".paperpaw/plugins/test/a.png"
    );
    assert_eq!(roots.resolve(20, 0, "a.png"), Err(ERR_DENIED));
    assert_eq!(roots.resolve(19, 1, "a.png"), Err(ERR_DENIED));
    assert_eq!(roots.resolve(19, 0, "../a.png"), Err(ERR_INVALID));
    assert_eq!(
        OwnerRoots::new(19, &plugin, true)
            .resolve(19, 1, "books/a.png")
            .unwrap(),
        "books/a.png"
    );
}

#[test]
fn image_turn_keeps_failed_delivery_and_retirement_discards_answers() {
    let pool = HostResources::try_new().unwrap();
    let owner = pool.try_owner().unwrap();
    let storage = MemoryStorage::new();
    pool.hosts()
        .images()
        .refuse(owner.owner_id(), 7, 4)
        .unwrap();
    assert_eq!(
        pool.image_turn(&storage, ImageProfile::x4(480, false), 10, &mut |_, _| Err(
            ERR_BUSY
        )),
        Err(ERR_BUSY)
    );
    let mut count = 0;
    assert_eq!(
        pool.image_turn(
            &storage,
            ImageProfile::x4(480, false),
            11,
            &mut |key, bytes| {
                assert_eq!(key, owner.owner_id());
                let record = super::super::registrations::records_decode_single(bytes).unwrap();
                let super::super::registrations::Record::EventImageReady(event) = record else {
                    panic!("wrong event")
                };
                assert_eq!((event.now_ms, event.cookie, event.status), (11, 7, 4));
                count += 1;
                Ok(())
            }
        ),
        Ok(false)
    );
    assert_eq!(count, 1);
    pool.hosts()
        .images()
        .refuse(owner.owner_id(), 8, 4)
        .unwrap();
    owner.release_all().unwrap();
    assert_eq!(
        pool.image_turn(
            &storage,
            ImageProfile::x4(480, false),
            12,
            &mut |_, _| panic!("stopped owner")
        ),
        Ok(false)
    );
}

#[test]
fn boot_sweep_only_removes_owned_cache_files() {
    let pool = HostResources::try_new().unwrap();
    let storage = MemoryStorage::new();
    storage
        .write(".paperpaw/cache/images/1-1.img", b"stale")
        .unwrap();
    storage
        .write(".paperpaw/cache/images/keep.txt", b"reader")
        .unwrap();
    let mut sweep = ImageSweep::default();
    assert_eq!(pool.sweep_image_cache(&storage, &mut sweep), Ok(true));
    assert_eq!(pool.sweep_image_cache(&storage, &mut sweep), Ok(false));
    assert!(storage.size(".paperpaw/cache/images/1-1.img").is_err());
    assert_eq!(storage.size(".paperpaw/cache/images/keep.txt").unwrap(), 6);
}

#[test]
fn full_queue_retains_command_order_while_turns_free_room() {
    use crate::native_host::image_service::commands::ImageCommand;
    use paperpaw_native::host_glue::ERR_FULL;
    let pool = HostResources::try_new().unwrap();
    let owner = pool.try_owner().unwrap();
    let storage = MemoryStorage::new();
    for cookie in 0..4 {
        pool.hosts()
            .images()
            .refuse(owner.owner_id(), cookie, 4)
            .unwrap();
    }
    let work = &pool.hosts().image_work;
    let batch = work
        .begin(owner.owner_id(), &Id::new_static("test"), false)
        .unwrap();
    assert!(matches!(
        work.begin(owner.owner_id(), &Id::new_static("test"), false),
        Err(ERR_BUSY)
    ));
    batch
        .finish(
            alloc::vec![5, 6]
                .into_iter()
                .map(|cookie| ImageCommand::Prepare {
                    root: 0,
                    path: "missing.png".into(),
                    side: 0,
                    cookie,
                    colour: false,
                })
                .collect(),
        )
        .unwrap();
    let mut cookies = alloc::vec::Vec::new();
    for _ in 0..8 {
        let result = pool.image_turn(
            &storage,
            ImageProfile::x4(480, false),
            0,
            &mut |key, bytes| {
                assert_eq!(key, owner.owner_id());
                let super::super::registrations::Record::EventImageReady(event) =
                    super::super::registrations::records_decode_single(bytes).unwrap()
                else {
                    panic!("wrong event")
                };
                cookies.push(event.cookie);
                Ok(())
            },
        );
        assert!(result.is_ok() || result == Err(ERR_FULL));
    }
    assert_eq!(cookies, [0, 1, 2, 3, 5, 6]);
    assert!(!work.pending());
}

#[test]
fn owner_retirement_cancels_retry_batches_and_aborted_callbacks_release_reservations() {
    use crate::native_host::image_service::commands::ImageCommand;
    let pool = HostResources::try_new().unwrap();
    let owner = pool.try_owner().unwrap();
    let plugin = Id::new_static("test");
    let work = &pool.hosts().image_work;
    drop(work.begin(owner.owner_id(), &plugin, false).unwrap());
    assert!(!work.pending());
    work.begin(owner.owner_id(), &plugin, false)
        .unwrap()
        .finish(alloc::vec![ImageCommand::Release { image: 9 }])
        .unwrap();
    assert!(work.pending());
    owner.release_all().unwrap();
    assert!(!work.pending());
}
