use super::*;
use crate::meter::{Meter, Metered};
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::Cell;
use paperpaw_platform::DirEntry;
use paperpaw_sdk::MemoryStorage;

const DIR: &str = ".paperpaw/plugins/battery/";

/// Memory storage with settable space and an injectable failure.
struct Fake {
    files: Rc<MemoryStorage>,
    free: Cell<Option<u64>>,
    total: Cell<Option<u64>>,
    fail: Cell<Option<StorageError>>,
    /// Folders must be made before a file in them (as on FAT): the folders
    /// made, and how often `create_dir` was called.
    strict: Cell<bool>,
    made: core::cell::RefCell<Vec<alloc::string::String>>,
    dir_calls: Cell<u32>,
}

impl Fake {
    fn new() -> Self {
        Self {
            files: Rc::new(MemoryStorage::new()),
            free: Cell::new(None),
            total: Cell::new(None),
            fail: Cell::new(None),
            strict: Cell::new(false),
            made: core::cell::RefCell::new(Vec::new()),
            dir_calls: Cell::new(0),
        }
    }
    fn check(&self) -> Result<(), StorageError> {
        self.fail.get().map_or(Ok(()), Err)
    }
    /// `NotFound` where strict and the folder above `p` was not made.
    fn folder_of(&self, p: &str) -> Result<(), StorageError> {
        let parent = p.rsplit_once('/').map_or("", |(d, _)| d);
        let made = self.made.borrow().iter().any(|d| d == parent);
        if self.strict.get() && !made {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }
}

impl Storage for Fake {
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
        self.check()?;
        self.files.read_at(p, o, b)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        self.check()?;
        self.files.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.check()?;
        self.folder_of(p)?;
        self.files.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.check()?;
        self.folder_of(p)?;
        self.files.append(p, d)
    }
    fn sync(&self) -> Result<(), StorageError> {
        self.check()
    }
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.files.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.check()?;
        self.files.remove(p)
    }
    fn rename(&self, f: &str, t: &str) -> Result<(), StorageError> {
        self.check()?;
        if !self.files.exists(f) {
            return Err(StorageError::NotFound);
        }
        self.folder_of(t)?;
        self.files.rename(f, t)
    }
    fn create_dir(&self, p: &str) -> Result<(), StorageError> {
        self.dir_calls.set(self.dir_calls.get() + 1);
        self.made.borrow_mut().push(p.into());
        self.files.create_dir(p)
    }
    fn free_space(&self) -> Option<u64> {
        self.free.get()
    }
    fn total_space(&self) -> Option<u64> {
        self.total.get()
    }
}

fn adapter(fake: &Fake) -> StorageAdapter<'_> {
    StorageAdapter::new(fake, "battery").unwrap()
}

fn get(fake: &Fake, name: &str) -> Option<Vec<u8>> {
    fake.files.get(&alloc::format!("{DIR}{name}"))
}

#[test]
fn write_read_size_append_rename_remove() {
    let fake = Fake::new();
    let s = adapter(&fake);
    assert_eq!(s.size(0, b"log.csv"), Err(ERR_NOT_FOUND));
    assert_eq!(s.write(0, b"sub/log.csv", b"abc"), Ok(()));
    assert_eq!(s.append(0, b"sub/log.csv", b"def"), Ok(()));
    assert_eq!(get(&fake, "sub/log.csv").unwrap(), b"abcdef");
    assert_eq!(s.size(0, b"sub/log.csv"), Ok(6));
    let mut out = [0u8; 4];
    assert_eq!(s.read_at(0, b"sub/log.csv", 4, &mut out), Ok(2));
    assert_eq!(&out[..2], b"ef");
    assert_eq!(s.read_at(0, b"sub/log.csv", 6, &mut out), Ok(0));
    assert_eq!(s.read_at(0, b"none", 0, &mut out), Err(ERR_NOT_FOUND));
    assert_eq!(s.rename(0, b"sub/log.csv", b"new/log.old.csv"), Ok(()));
    assert_eq!(s.rename(0, b"sub/log.csv", b"x"), Err(ERR_NOT_FOUND));
    assert!(get(&fake, "sub/log.csv").is_none());
    assert_eq!(get(&fake, "new/log.old.csv").unwrap(), b"abcdef");
    assert_eq!(s.remove(0, b"new/log.old.csv"), Ok(()));
    assert_eq!(s.remove(0, b"new/log.old.csv"), Ok(()));
    assert_eq!(s.write(0, b"empty", b""), Ok(()));
    assert_eq!(get(&fake, "empty").unwrap(), b"");
    assert_eq!(s.append(0, b"made", b""), Ok(()));
    assert_eq!(s.sync(), Ok(()));
}

#[test]
fn roots_other_than_zero_are_reserved() {
    let fake = Fake::new();
    let s = adapter(&fake);
    let mut out = [0u8; 1];
    for root in [1, 2, u32::MAX] {
        assert_eq!(s.size(root, b"a"), Err(ERR_UNSUPPORTED));
        assert_eq!(s.read_at(root, b"a", 0, &mut out), Err(ERR_UNSUPPORTED));
        assert_eq!(s.write(root, b"a", b"x"), Err(ERR_UNSUPPORTED));
        assert_eq!(s.append(root, b"a", b"x"), Err(ERR_UNSUPPORTED));
        assert_eq!(s.rename(root, b"a", b"b"), Err(ERR_UNSUPPORTED));
        assert_eq!(s.remove(root, b"a"), Err(ERR_UNSUPPORTED));
    }
    assert!(get(&fake, "a").is_none());
}

#[test]
fn paths_cannot_leave_the_data_folder() {
    let fake = Fake::new();
    fake.files
        .write("plugins/battery/plugin.ini", b"x")
        .unwrap();
    let s = adapter(&fake);
    for bad in [
        &b"../x"[..],
        b"a/../../x",
        b"/plugins/battery/plugin.ini",
        b"/x",
        b"./x",
        b"a/./x",
        b"a//x",
        b"a/",
        b"",
        b"a\\b",
        b"c:x",
        b"a\0b",
        b"a\nb",
        b"x.",
        b"con",
        b"..",
    ] {
        assert_eq!(s.write(0, bad, b"x"), Err(ERR_INVALID), "{bad:?}");
        assert_eq!(s.size(0, bad), Err(ERR_INVALID));
        assert_eq!(s.remove(0, bad), Err(ERR_INVALID));
        assert_eq!(s.rename(0, bad, b"ok"), Err(ERR_INVALID));
        assert_eq!(s.rename(0, b"ok", bad), Err(ERR_INVALID));
    }
    assert_eq!(s.write(0, &[0xff, 0xfe], b"x"), Err(ERR_UTF8));
    // The install folder is only reachable below the data folder.
    assert_eq!(s.write(0, b"plugins/battery/plugin.ini", b"y"), Ok(()));
    assert_eq!(fake.files.get("plugins/battery/plugin.ini").unwrap(), b"x");
    assert_eq!(get(&fake, "plugins/battery/plugin.ini").unwrap(), b"y");
    assert_eq!(fake.files.get("x"), None);
}

#[test]
fn caps_are_refused_before_anything_happens() {
    let fake = Fake::new();
    let s = adapter(&fake);
    let long = [b'a'; MAX_PATH + 1];
    assert_eq!(s.write(0, &long, b"x"), Err(ERR_NO_SPACE));
    let seg = [b'a'; MAX_SEGMENT + 1];
    assert_eq!(s.write(0, &seg, b"x"), Err(ERR_NO_SPACE));
    assert_eq!(s.write(0, &seg[..MAX_SEGMENT], b"x"), Ok(()));
    let big = [0u8; MAX_DATA + 1];
    assert_eq!(s.write(0, b"f", &big), Err(ERR_NO_SPACE));
    assert_eq!(s.append(0, b"f", &big), Err(ERR_NO_SPACE));
    assert_eq!(s.write(0, b"f", &big[..MAX_DATA]), Ok(()));
    let mut out = [0u8; MAX_DATA + 1];
    assert_eq!(s.read_at(0, b"f", 0, &mut out), Err(ERR_NO_SPACE));
    assert_eq!(s.read_at(0, b"f", 0, &mut out[..MAX_DATA]), Ok(MAX_DATA));
    // The longest path in the longest segments still fits the joined buffer.
    let mut deep = Vec::new();
    for _ in 0..3 {
        deep.extend_from_slice(&[b'd'; 127]);
        deep.push(b'/');
    }
    deep.extend_from_slice(b"f");
    assert!(deep.len() <= MAX_PATH);
    assert_eq!(s.write(0, &deep, b"x"), Ok(()));
    assert!(StorageAdapter::new(&fake, "bad id").is_none());
}

#[test]
fn a_full_card_writes_nothing() {
    let fake = Fake::new();
    let s = adapter(&fake);
    s.write(0, b"keep", b"old").unwrap();
    fake.total.set(Some(8 << 30));
    let reserve = (8u64 << 30) / 100;
    // Exactly the reserve stays: allowed. One byte more: refused.
    fake.free.set(Some(reserve + 10));
    assert_eq!(s.write(0, b"keep", &[1u8; 10]), Ok(()));
    fake.free.set(Some(reserve + 9));
    assert_eq!(s.write(0, b"keep", &[2u8; 10]), Err(ERR_FULL));
    assert_eq!(s.append(0, b"keep", &[2u8; 10]), Err(ERR_FULL));
    assert_eq!(s.write(0, b"new", &[3u8; 20]), Err(ERR_FULL));
    assert_eq!(get(&fake, "keep").unwrap(), [1u8; 10]);
    assert!(get(&fake, "new").is_none());
    // Small cards keep 256 KiB.
    fake.total.set(Some(10 << 20));
    fake.free.set(Some(MIN_RESERVE + 1));
    assert_eq!(s.append(0, b"keep", b"ab"), Err(ERR_FULL));
    assert_eq!(s.append(0, b"keep", b"a"), Ok(()));
    fake.total.set(None);
    fake.free.set(Some(MIN_RESERVE - 1));
    assert_eq!(s.append(0, b"keep", b""), Err(ERR_FULL));
    // Reading, renaming and removing stay possible on a full card.
    assert_eq!(s.rename(0, b"keep", b"kept"), Ok(()));
    assert_eq!(s.remove(0, b"kept"), Ok(()));
    // Unknown space: the storage decides.
    fake.free.set(None);
    assert_eq!(s.write(0, b"keep", b"x"), Ok(()));
    fake.fail.set(Some(StorageError::Full));
    assert_eq!(s.write(0, b"keep", b"x"), Err(ERR_FULL));
}

#[test]
fn storage_failures_become_statuses() {
    let fake = Fake::new();
    let s = adapter(&fake);
    s.write(0, b"f", b"x").unwrap();
    let mut out = [0u8; 1];
    for (error, status) in [
        (StorageError::Io, ERR_IO),
        (StorageError::CARD_GONE, ERR_IO),
        (StorageError::InvalidData, ERR_IO),
        (StorageError::Unsupported, ERR_UNSUPPORTED),
        (StorageError::TooLarge, ERR_NO_SPACE),
        (StorageError::Full, ERR_FULL),
    ] {
        fake.fail.set(Some(error));
        assert_eq!(s.size(0, b"f"), Err(status));
        assert_eq!(s.read_at(0, b"f", 0, &mut out), Err(status));
        assert_eq!(s.write(0, b"f", b"y"), Err(status));
        assert_eq!(s.append(0, b"f", b"y"), Err(status));
        assert_eq!(s.rename(0, b"f", b"g"), Err(status));
        assert_eq!(s.remove(0, b"f"), Err(status));
        assert_eq!(s.sync(), Err(status));
    }
    fake.fail.set(None);
    assert_eq!(get(&fake, "f").unwrap(), b"x");
}

/// S1: remove the temp file first, append, sync, rename.
#[test]
fn a_large_write_from_a_leftover_temp_file_publishes_only_the_new_bytes() {
    let fake = Fake::new();
    let s = adapter(&fake);
    s.write(0, b"data.bin", b"published").unwrap();
    s.write(0, b"data.bin.tmp", b"STALE-STALE-STALE").unwrap();
    assert_eq!(s.remove(0, b"data.bin.tmp"), Ok(()));
    for chunk in [&b"new-"[..], b"bytes", b"!"] {
        assert_eq!(s.append(0, b"data.bin.tmp", chunk), Ok(()));
    }
    assert_eq!(s.sync(), Ok(()));
    assert_eq!(s.rename(0, b"data.bin.tmp", b"data.bin"), Ok(()));
    assert_eq!(get(&fake, "data.bin").unwrap(), b"new-bytes!");
    assert!(get(&fake, "data.bin.tmp").is_none());
}

#[test]
fn use_is_counted_for_the_plugin_that_runs() {
    let fake: Box<dyn Storage> = Box::new(Fake::new());
    let meter = Meter::new(None);
    let metered = Metered {
        inner: fake,
        meter: meter.clone(),
    };
    let s = StorageAdapter::new(&metered, "battery").unwrap();
    {
        let _running = meter.enter("battery");
        s.write(0, b"f", b"abcd").unwrap();
        s.append(0, b"f", b"ef").unwrap();
        let mut out = [0u8; 8];
        assert_eq!(s.read_at(0, b"f", 0, &mut out), Ok(6));
        s.sync().unwrap();
        s.sync().unwrap();
        // Refused calls count nothing.
        assert!(s.write(0, b"../f", b"abcd").is_err());
        assert!(s.write(1, b"f", b"abcd").is_err());
    }
    s.sync().unwrap();
    let table = meter.table.borrow().clone();
    let row = |id: &str| table.iter().find(|p| p.plugin.as_str() == id).unwrap();
    assert_eq!((row("battery").read, row("battery").written), (6, 6));
    assert_eq!(meter.syncs_of("battery"), 2);
    assert_eq!(meter.syncs_of("core"), 1);
}

/// Folders are made only where the storage says one is missing: a file in a
/// folder that exists (every append after the first) costs no `create_dir`.
#[test]
fn folders_are_made_only_when_missing() {
    let fake = Fake::new();
    fake.strict.set(true);
    let s = adapter(&fake);
    assert_eq!(s.append(0, b"log.csv", b"a\n"), Ok(()));
    assert_eq!(fake.dir_calls.get(), 1);
    for _ in 0..3 {
        assert_eq!(s.append(0, b"log.csv", b"b\n"), Ok(()));
    }
    assert_eq!(s.write(0, b"state.ini", b"x"), Ok(()));
    assert_eq!(fake.dir_calls.get(), 1);
    assert_eq!(s.rename(0, b"log.csv", b"old/log.csv"), Ok(()));
    assert_eq!(fake.dir_calls.get(), 2);
    assert_eq!(get(&fake, "old/log.csv").unwrap(), b"a\nb\nb\nb\n");
    // A missing source stays NotFound, after one try to make the folder.
    assert_eq!(s.rename(0, b"none", b"new/x"), Err(ERR_NOT_FOUND));
}
