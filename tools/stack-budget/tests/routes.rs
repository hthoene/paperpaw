//! The X4's deep routes on a thread with the device's stack (#256): what the
//! static scans (`tools/check-x4-start-stack.py`, `frame-budget-s3*.txt`) cannot
//! follow, because it goes through function pointers: the FAT under every
//! storage call of a plugin's host service, the tree removal, and the
//! picture-folder provider (`plugins/format-images`) on a FAT card.
//!
//! Each route runs on the stack of the tightest device ([`S3`]) with a
//! [`Meter`] at its leaves: every block the card reads or writes (the FAT's
//! deepest frames) and every call the provider makes into its storage (a
//! decoder reading its file, a cache write). The ceilings are the depths
//! measured on the host (`cargo test -p paperpaw-stack-budget -- --nocapture`
//! prints them) plus room; see the crate's docs for what the host's bytes mean.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_plugin_format_images::CACHE;
use paperpaw_plugin_format_images::provider::ImagesProvider;
use paperpaw_sdk::native::stage_schemas::core_format_v1 as stage;
use paperpaw_sdk::{DirEntry, Storage, StorageError};
use paperpaw_stack_budget::{DEVICES, Meter, Route, S3, X4, deepen, on_device_stack};

use paperpaw_stack_budget::card::{Fat, fat as card};

/// The ceilings: the depth measured on the host (`cargo test -p
/// paperpaw-stack-budget -- --nocapture` prints them; 5,216, 5,376 and 8,464
/// bytes when they were set) and a fifth more, rounded up. A change that goes
/// past one is a route deeper than it was: look at what grew before raising it.
const CEIL_FAT_CALLS: usize = 6400;
const CEIL_TREE: usize = 6656;
const CEIL_IMAGES: usize = 10240;

/// Runs `route` on the tightest stack and reports its depth; the card's
/// set-up is before the meter starts.
fn measured(route: Route, body: impl FnOnce(&Rc<Meter>) + Send) -> usize {
    on_device_stack(S3, || {
        let meter = Rc::new(Meter::new());
        body(&meter);
        let depth = route.assert_within(&meter);
        println!(
            "stack route {}: {depth} B (ceiling {})",
            route.name, route.ceiling
        );
        depth
    })
}

fn files(storage: &dyn Storage, dir: &str, count: usize) {
    for i in 0..count {
        // Long names: the long-name entries of FAT are part of the route.
        storage
            .write(&format!("{dir}/a-long-file-name-number-{i:04}.epub"), b"x")
            .unwrap();
    }
}

/// Every call a plugin's files host makes on a FAT card: list in pages, stat,
/// read, write, append, remove.
#[test]
fn fat_storage_calls() {
    let route = Route {
        name: "fat: list, stat, read, write, append, remove",
        ceiling: CEIL_FAT_CALLS,
    };
    measured(route, |meter| {
        let fat = card(meter);
        files(fat, "Books", 60);
        meter.start();
        let mut from = 0;
        loop {
            let mut seen = 0;
            let next = fat
                .list_from("Books", from, &mut |_: DirEntry<'_>| {
                    seen += 1;
                    seen < 20
                })
                .unwrap();
            match next {
                Some(place) => from = place,
                None => break,
            }
        }
        fat.size("Books/a-long-file-name-number-0007.epub").unwrap();
        let mut buf = [0u8; 4096];
        fat.read_at("Books/a-long-file-name-number-0007.epub", 0, &mut buf)
            .unwrap();
        fat.write("Books/new/deeper/file.txt", &[7u8; 5000])
            .unwrap();
        fat.append("Books/new/deeper/file.txt", &[8u8; 5000])
            .unwrap();
        fat.remove("Books/new/deeper/file.txt").unwrap();
    });
}

/// The bounded removal of a tree 24 folders deep (`remove_tree_step`, called
/// by the files host until it is done).
#[test]
fn fat_tree_removal() {
    let route = Route {
        name: "fat: remove_tree_step over a 24-deep tree",
        ceiling: CEIL_TREE,
    };
    measured(route, |meter| {
        let fat = card(meter);
        let mut dir = String::from("Tree");
        for level in 0..24 {
            files(fat, &dir, 3);
            dir = format!("{dir}/level-{level:02}-folder");
        }
        files(fat, &dir, 3);
        meter.start();
        let mut place = 0;
        for _ in 0..2000 {
            let step = fat.remove_tree_step("Tree", false, place).unwrap();
            if step.done {
                return;
            }
            place = step.next;
        }
        panic!("the tree was not removed in 2000 steps");
    });
}

/// The pictures `core.format` provider on a folder of a FAT card: open a
/// picture, count the pages, show each (decoded into the cache), the cover.
#[test]
fn picture_folder_on_fat() {
    const PNG: &[u8] = include_bytes!("../../../crates/paperpaw-image/tests/data/gray.png");
    const JPG: &[u8] = include_bytes!("../../../crates/paperpaw-image/tests/data/alice-cover.jpg");
    const GIF: &[u8] = include_bytes!("../../../crates/paperpaw-image/tests/data/alice-cover.gif");

    /// Root 3 (the book's folder) and root 6 (`cache:`) on the card, each
    /// call into the storage a leaf of the route.
    struct Roots(&'static Fat, Rc<Meter>);
    impl Roots {
        fn map(path: &str) -> String {
            match path.strip_prefix(CACHE) {
                Some(name) => format!("cache/{name}"),
                None => format!("book/{path}"),
            }
        }
    }
    impl Storage for Roots {
        fn read_at(&self, path: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
            self.1.mark();
            self.0.read_at(&Self::map(path), at, buf)
        }
        fn size(&self, path: &str) -> Result<u64, StorageError> {
            self.1.mark();
            self.0.size(&Self::map(path))
        }
        fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            self.1.mark();
            self.0.write(&Self::map(path), data)
        }
        fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            self.1.mark();
            self.0.append(&Self::map(path), data)
        }
        fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            self.1.mark();
            let dir = format!("book{}{dir}", if dir.is_empty() { "" } else { "/" });
            self.0.list(&dir, &mut |entry| {
                self.1.mark();
                visit(entry);
            })
        }
        fn remove(&self, path: &str) -> Result<(), StorageError> {
            self.1.mark();
            self.0.remove(&Self::map(path))
        }
    }

    fn call(op: u32, document: u32, f: impl FnOnce(&mut stage::FmtCall<'static>)) -> Vec<u8> {
        let mut c = stage::FmtCall::new(op, document, 0, 0, 0, 0, 0, 0, 0, 0, 7, "", &[]);
        f(&mut c);
        let mut buf = vec![0u8; 4096];
        let n = stage::input_encode(&[stage::Record::FmtCall(c)], &mut buf).unwrap();
        buf.truncate(n);
        buf
    }
    fn run(p: &mut ImagesProvider, io: &dyn Storage, input: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; 3840];
        let n = p.call(io, input, &mut out).unwrap();
        out.truncate(n);
        out
    }

    let route = Route {
        name: "images: a folder of pictures on a FAT card",
        ceiling: CEIL_IMAGES,
    };
    measured(route, |meter| {
        let fat = card(meter);
        fat.write("book/1.png", PNG).unwrap();
        fat.write("book/2.jpg", JPG).unwrap();
        fat.write("book/10.gif", GIF).unwrap();
        fat.write("book/readme.txt", b"hello").unwrap();
        fat.write("cache/.keep", b"").unwrap();
        let io = Roots(fat, Rc::clone(meter));
        meter.start();
        let mut p = ImagesProvider::new();
        run(&mut p, &io, &call(2, 1, |c| c.text = "1.png"));
        run(&mut p, &io, &call(7, 1, |_| {}));
        for page in 0..3 {
            run(&mut p, &io, &call(8, 1, |c| c.section = page));
            run(&mut p, &io, &call(18, 1, |c| c.index = page));
        }
        run(&mut p, &io, &call(17, 1, |c| c.cover_side = 120));
        run(&mut p, &io, &call(5, 1, |_| {}));
    });
}

/// The check fails where a route is made deeper: the FAT listing first as it
/// is (its depth plus 256 B is the ceiling, and it passes), then with a leaf
/// 1 KiB further down, which the same ceiling refuses.
#[test]
fn a_route_made_deeper_is_refused() {
    fn listing(extra: usize) -> usize {
        on_device_stack(S3, || {
            let meter = Rc::new(Meter::new());
            let fat = card(&meter);
            files(fat, "Books", 3);
            meter.start();
            deepen(extra, &mut || fat.list("Books", &mut |_| {}).unwrap());
            meter.deepest()
        })
    }
    let depth = listing(0);
    let route = Route {
        name: "fat: list, deepened on purpose",
        ceiling: depth + 256,
    };
    assert_eq!(route.verdict_of(depth), Ok(depth));
    let over = route.verdict_of(listing(1024)).unwrap_err();
    assert_eq!((over.route, over.bound), (route.name, depth + 256));
    assert!(over.depth >= depth + 1024, "{over}");
}

/// A route that stays inside its ceiling but not inside a device's stack is
/// refused with the device named: the S3's is tighter than the X4's.
#[test]
fn a_route_over_a_devices_stack_is_refused() {
    let meter = Meter::new();
    meter.start();
    deepen(S3.usable + 1024, &mut || meter.mark());
    // The stack of the test thread is 2 MiB: only the verdict is under test.
    let route = Route {
        name: "synthetic",
        ceiling: usize::MAX,
    };
    let over = route.verdict(&meter).unwrap_err();
    assert_eq!(over.against, S3.name, "{over}");
}

/// The device numbers here are the firmware's own (the constants this crate
/// cannot depend on: `paperpaw_x4` builds the firmware's crates).
#[test]
fn the_devices_match_the_firmware_constants() {
    use paperpaw_x4::{s3_band, stack_band};
    assert_eq!(
        paperpaw_stack_budget::STACK_FLOOR as u32,
        s3_band::STACK_FLOOR
    );
    assert_eq!(
        X4.usable as u32,
        s3_band::STACK_FLOOR
            - stack_band::RESERVE
            - stack_band::BAND
            - (stack_band::BAND - 1)
            - paperpaw_stack_budget::MARGIN as u32
    );
    assert_eq!(
        S3.usable as u32,
        s3_band::STACK_FLOOR
            - s3_band::RESERVE
            - s3_band::BAND
            - 15
            - paperpaw_stack_budget::MARGIN as u32
    );
    assert_eq!(DEVICES, [X4, S3]);
}
