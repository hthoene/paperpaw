//! The mount owns its allocation even with cached and dirty FAT handles.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests fail immediately on unexpected storage errors"
)]

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::Bytes;
use paperpaw_fat::fatfs::{FileSystem, FormatVolumeOptions, FsOptions};
use paperpaw_fat::{BlockDisk, FatStorage, SharedDisk};
use paperpaw_platform::Storage;

fn formatted() -> Bytes {
    let bytes = Bytes(Rc::new(RefCell::new(vec![0; 4 << 20])));
    let mut disk = SharedDisk::new(BlockDisk::new(bytes.clone()).unwrap());
    paperpaw_fat::fatfs::format_volume(&mut disk, FormatVolumeOptions::new()).unwrap();
    disk.flush_all().unwrap();
    bytes
}

fn mount(bytes: &Bytes) -> FatStorage<SharedDisk<Bytes>> {
    let disk = SharedDisk::new(BlockDisk::new(bytes.clone()).unwrap());
    let fs = FileSystem::new(disk.clone(), FsOptions::new()).unwrap();
    FatStorage::owned(fs).with_disk(disk)
}

#[test]
fn repeated_mounts_release_the_disk_with_live_and_dirty_handles() {
    let bytes = formatted();
    for round in 0..if cfg!(miri) { 3 } else { 64 } {
        let storage = mount(&bytes);
        for n in 0..20 {
            let path = format!("books/shelf{n}/book.txt");
            storage.write(&path, b"book").unwrap();
            storage.append(&path, b"note").unwrap();
        }
        storage.sync().unwrap();
        // A move of the storage does not invalidate the cached borrows.
        let moved = Box::new(storage);
        assert_eq!(moved.size("books/shelf19/book.txt"), Ok(8));
        moved.append("books/shelf19/book.txt", b"dirty").unwrap();
        drop(moved);
        // The filesystem's IO is the last owner of its block cache.
        // Leaking the filesystem keeps this reference after every mount.
        assert_eq!(Rc::strong_count(&bytes.0), 1, "round {round}");
    }
}

#[test]
fn a_visit_that_unwinds_still_releases_the_mount() {
    let bytes = formatted();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let storage = mount(&bytes);
        storage.write("books/a.txt", b"a").unwrap();
        storage
            .list("books", &mut |_| panic!("visitor stopped"))
            .unwrap();
    }));
    assert!(result.is_err());
    assert_eq!(Rc::strong_count(&bytes.0), 1);
}

#[test]
fn a_failed_mount_releases_its_disk() {
    let bytes = Bytes(Rc::new(RefCell::new(vec![0; 4 << 20])));
    for _ in 0..32 {
        let disk = SharedDisk::new(BlockDisk::new(bytes.clone()).unwrap());
        assert!(FileSystem::new(disk, FsOptions::new()).is_err());
        assert_eq!(Rc::strong_count(&bytes.0), 1);
    }
}
