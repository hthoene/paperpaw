//! The part file on a storage that allocates nothing of its own, so that
//! only the plugin's allocations are refused in turn.
use super::*;
use core::cell::RefCell;

const CAP: usize = 64 * 1024;

/// Two files in fixed arrays; no allocation. A `full` card refuses writes.
struct Fixed {
    files: RefCell<[Slot; 2]>,
    full_after: RefCell<Option<usize>>,
}

struct Slot {
    name: [u8; 64],
    name_len: usize,
    data: [u8; CAP],
    len: usize,
    used: bool,
}

impl Slot {
    const fn empty() -> Self {
        Self {
            name: [0; 64],
            name_len: 0,
            data: [0; CAP],
            len: 0,
            used: false,
        }
    }
    fn is(&self, name: &str) -> bool {
        self.used && self.name.get(..self.name_len) == Some(name.as_bytes())
    }
}

impl Fixed {
    fn new() -> Self {
        Self {
            files: RefCell::new([Slot::empty(), Slot::empty()]),
            full_after: RefCell::new(None),
        }
    }
    fn with(&self, name: &str, data: &[u8]) {
        self.write(name, data).unwrap();
    }
    fn get(&self, name: &str) -> Option<Vec<u8>> {
        let files = self.files.borrow();
        let slot = files.iter().find(|s| s.is(name))?;
        slot.data.get(..slot.len).map(<[u8]>::to_vec)
    }
    /// Whether `name` holds exactly `data`: no allocation.
    fn holds(&self, name: &str, data: &[u8]) -> bool {
        let files = self.files.borrow();
        files
            .iter()
            .find(|s| s.is(name))
            .is_some_and(|s| s.data.get(..s.len) == Some(data))
    }
    fn names(&self) -> usize {
        self.files.borrow().iter().filter(|s| s.used).count()
    }
    fn room(&self, more: usize) -> Result<(), StorageError> {
        let mut left = self.full_after.borrow_mut();
        match left.as_mut() {
            Some(n) if *n < more => Err(StorageError::Full),
            Some(n) => {
                *n -= more;
                Ok(())
            }
            None => Ok(()),
        }
    }
}

impl Storage for Fixed {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let files = self.files.borrow();
        let slot = files
            .iter()
            .find(|s| s.is(path))
            .ok_or(StorageError::NotFound)?;
        let from = usize::try_from(offset).unwrap_or(usize::MAX).min(slot.len);
        let n = buf.len().min(slot.len - from);
        buf[..n].copy_from_slice(&slot.data[from..from + n]);
        Ok(n)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        let files = self.files.borrow();
        let slot = files
            .iter()
            .find(|s| s.is(path))
            .ok_or(StorageError::NotFound)?;
        Ok(slot.len as u64)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.room(data.len())?;
        let mut files = self.files.borrow_mut();
        let at = match files.iter().position(|s| s.is(path)) {
            Some(at) => at,
            None => files
                .iter()
                .position(|s| !s.used)
                .ok_or(StorageError::Full)?,
        };
        let slot = &mut files[at];
        slot.name[..path.len()].copy_from_slice(path.as_bytes());
        slot.name_len = path.len();
        slot.used = true;
        slot.data
            .get_mut(..data.len())
            .ok_or(StorageError::Full)?
            .copy_from_slice(data);
        slot.len = data.len();
        Ok(())
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.room(data.len())?;
        let mut files = self.files.borrow_mut();
        let slot = files
            .iter_mut()
            .find(|s| s.is(path))
            .ok_or(StorageError::NotFound)?;
        let end = slot.len + data.len();
        slot.data
            .get_mut(slot.len..end)
            .ok_or(StorageError::Full)?
            .copy_from_slice(data);
        slot.len = end;
        Ok(())
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Ok(())
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        let mut files = self.files.borrow_mut();
        let slot = files
            .iter_mut()
            .find(|s| s.is(path))
            .ok_or(StorageError::NotFound)?;
        slot.used = false;
        Ok(())
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let mut files = self.files.borrow_mut();
        if !files.iter().any(|s| s.is(from)) {
            return Err(StorageError::NotFound);
        }
        // The new name replaces a file of that name.
        if let Some(old) = files.iter_mut().find(|s| s.is(to)) {
            old.used = false;
        }
        let slot = files.iter_mut().find(|s| s.is(from)).unwrap();
        slot.name[..to.len()].copy_from_slice(to.as_bytes());
        slot.name_len = to.len();
        Ok(())
    }
    fn create_dir(&self, _: &str) -> Result<(), StorageError> {
        Ok(())
    }
}

use paperpaw_sdk::DirEntry;

const FILE: &str = "Exports/My Clippings.txt";

/// The text of an export, made once before anything is refused: more than two
/// pieces, so that the file is appended to.
fn text() -> String {
    let mut text = String::new();
    for n in 0..70 {
        text.push_str(&alloc::format!("line {n} of the notes of a book\r\n"));
        text.push_str(&"x".repeat(300));
        text.push_str("\r\n==========\r\n");
    }
    text
}

fn write(storage: &Fixed, text: &str) -> Result<(), Error> {
    let mut file = PartFile::new(storage, "Exports", "My Clippings.txt")?;
    for piece in text.as_bytes().chunks(997) {
        file.put(core::str::from_utf8(piece).unwrap_or(""))?;
    }
    file.finish()
}

#[test]
fn a_file_is_written_in_pieces_and_put_in_place_whole() {
    let storage = Fixed::new();
    let text = text();
    assert!(text.len() > 2 * FLUSH);
    storage.with(FILE, b"the old file");
    write(&storage, &text).unwrap();
    assert_eq!(storage.get(FILE).unwrap(), text.as_bytes());
    assert_eq!(storage.names(), 1, "the part is gone");
}

#[test]
fn every_refused_allocation_leaves_the_old_file_and_no_part() {
    let text = text();
    let storage = Fixed::new();
    let total = crate::fail_alloc::sweep(|| {
        storage.with(FILE, b"the old file");
        let result = write(&storage, &text);
        match &result {
            Ok(()) => assert!(storage.holds(FILE, text.as_bytes())),
            Err(_) => {
                assert!(storage.holds(FILE, b"the old file"));
                assert_eq!(storage.names(), 1, "a part is left");
            }
        }
        result
    });
    assert!(total >= 3, "the sweep proves nothing: {total}");
}

#[test]
fn a_full_card_is_told_and_leaves_the_old_file() {
    let text = text();
    // Room for the first piece or two only, then none.
    for room in [0, FLUSH, 2 * FLUSH, text.len() - 1] {
        let storage = Fixed::new();
        storage.with(FILE, b"the old file");
        *storage.full_after.borrow_mut() = Some(room);
        assert_eq!(write(&storage, &text), Err(Error::Full), "room {room}");
        assert_eq!(storage.get(FILE).unwrap(), b"the old file");
        assert_eq!(storage.names(), 1, "a part is left at room {room}");
    }
}

#[test]
fn a_new_file_is_made_where_there_was_none() {
    let storage = Fixed::new();
    write(&storage, "short").unwrap();
    assert_eq!(storage.get(FILE).unwrap(), b"short");
}

#[test]
fn a_file_dropped_before_the_end_leaves_no_part() {
    let storage = Fixed::new();
    {
        let mut file = PartFile::new(&storage, "Exports", "My Clippings.txt").unwrap();
        file.put(&"y".repeat(FLUSH + 10)).unwrap();
        assert_eq!(storage.names(), 1, "the part is there");
    }
    assert_eq!(storage.names(), 0);
}
