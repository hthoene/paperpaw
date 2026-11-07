//! Whole files of a plugin's data folder over the 4 KiB storage calls.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    storage::{self, MAX_DATA, Root},
};

/// The ending of the name a file is written under before it replaces the old.
const TEMP: &str = ".new";

/// The storage calls a file is made of: the host's, or a stand-in in tests.
pub trait Volume {
    /// The size of a file.
    fn size(&self, name: &str) -> Result<u64, Error>;
    /// Reads at an offset.
    fn read_at(&self, name: &str, at: u64, out: &mut [u8]) -> Result<usize, Error>;
    /// Writes a file whole (at most [`MAX_DATA`] bytes), replacing it atomically.
    fn write(&self, name: &str, data: &[u8]) -> Result<(), Error>;
    /// Appends to a file.
    fn append(&self, name: &str, data: &[u8]) -> Result<(), Error>;
    /// Renames over `to`.
    fn rename(&self, from: &str, to: &str) -> Result<(), Error>;
    /// Removes a file.
    fn remove(&self, name: &str) -> Result<(), Error>;
    /// Flushes.
    fn sync(&self) -> Result<(), Error>;
}

struct OnHost<'a, 'h> {
    host: &'a Host<'h>,
    root: Root,
}

impl Volume for OnHost<'_, '_> {
    fn size(&self, name: &str) -> Result<u64, Error> {
        storage::size(self.host, self.root, name)
    }
    fn read_at(&self, name: &str, at: u64, out: &mut [u8]) -> Result<usize, Error> {
        storage::read_at(self.host, self.root, name, at, out)
    }
    fn write(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        storage::write(self.host, self.root, name, data)
    }
    fn append(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        storage::append(self.host, self.root, name, data)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), Error> {
        storage::rename(self.host, self.root, from, to)
    }
    fn remove(&self, name: &str) -> Result<(), Error> {
        storage::remove(self.host, self.root, name)
    }
    fn sync(&self) -> Result<(), Error> {
        storage::sync(self.host)
    }
}

/// A file of at most `max` bytes: `None` where there is none.
///
/// # Errors
/// [`Error::NoSpace`] for a larger file, [`Error::NoMemory`] when it does not
/// fit in memory, or the host's refusal.
pub fn read_all(
    host: &Host<'_>,
    root: Root,
    name: &str,
    max: usize,
) -> Result<Option<Vec<u8>>, Error> {
    read_all_on(&OnHost { host, root }, name, max)
}

/// The first `n` bytes of a file (fewer where it is shorter): `None` where there
/// is none.
///
/// # Errors
/// [`Error::NoMemory`] or the host's refusal.
pub fn read_head(
    host: &Host<'_>,
    root: Root,
    name: &str,
    n: usize,
) -> Result<Option<Vec<u8>>, Error> {
    read_head_on(&OnHost { host, root }, name, n)
}

/// Replaces a file with `bytes`, whole or not at all: a file beyond one call is
/// written under another name, flushed and renamed over the old.
///
/// # Errors
/// The host's refusal; the old file is intact then.
pub fn replace(host: &Host<'_>, root: Root, name: &str, bytes: &[u8]) -> Result<(), Error> {
    replace_on(&OnHost { host, root }, name, bytes)
}

fn read_head_on(v: &impl Volume, name: &str, n: usize) -> Result<Option<Vec<u8>>, Error> {
    let size = match v.size(name) {
        Ok(size) => usize::try_from(size).unwrap_or(usize::MAX),
        Err(Error::NotFound) => return Ok(None),
        Err(error) => return Err(error),
    };
    let n = n.min(size).min(MAX_DATA);
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(n).map_err(|_| Error::NoMemory)?;
    bytes.resize(n, 0);
    let got = v.read_at(name, 0, &mut bytes)?;
    bytes.truncate(got);
    Ok(Some(bytes))
}

fn read_all_on(v: &impl Volume, name: &str, max: usize) -> Result<Option<Vec<u8>>, Error> {
    let size = match v.size(name) {
        Ok(size) => usize::try_from(size).map_err(|_| Error::NoSpace)?,
        Err(Error::NotFound) => return Ok(None),
        Err(error) => return Err(error),
    };
    if size > max {
        return Err(Error::NoSpace);
    }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(size).map_err(|_| Error::NoMemory)?;
    bytes.resize(size, 0);
    let mut at = 0;
    while at < size {
        let end = at.saturating_add(MAX_DATA).min(size);
        let n = v.read_at(name, at as u64, &mut bytes[at..end])?;
        if n == 0 || n > end - at {
            // The file shrank under the read, or the host overran the buffer.
            return Err(Error::Io);
        }
        at += n;
    }
    Ok(Some(bytes))
}

fn temp_name(name: &str) -> Result<String, Error> {
    let mut temp = String::new();
    temp.try_reserve_exact(name.len() + TEMP.len())
        .map_err(|_| Error::NoMemory)?;
    temp.push_str(name);
    temp.push_str(TEMP);
    Ok(temp)
}

fn replace_on(v: &impl Volume, name: &str, bytes: &[u8]) -> Result<(), Error> {
    if bytes.len() <= MAX_DATA {
        v.write(name, bytes)?;
        // A temp file a cut or a refusal left behind earlier is of no use now.
        if let Ok(temp) = temp_name(name) {
            let _ = v.remove(&temp);
        }
        return Ok(());
    }
    let temp = temp_name(name)?;
    let mut parts = bytes.chunks(MAX_DATA);
    let done = (|| {
        v.write(&temp, parts.next().unwrap_or(&[]))?;
        for part in parts {
            v.append(&temp, part)?;
        }
        v.sync()?;
        v.rename(&temp, name)
    })();
    if done.is_err() {
        let _ = v.remove(&temp);
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{collections::BTreeMap, vec};
    use core::cell::{Cell, RefCell};

    /// Storage in memory. Operation `fail_at` (counting from 1) is refused: a
    /// refusal when `cut` is false, a power cut (it and every later one refused,
    /// nothing applied) when it is true. A write or rename is whole or not at all,
    /// as the host's are.
    #[derive(Default)]
    struct Fake {
        files: RefCell<BTreeMap<String, Vec<u8>>>,
        ops: Cell<usize>,
        fail_at: Cell<usize>,
        cut: Cell<bool>,
        /// Reads give at most this many bytes.
        short: Cell<usize>,
        calls: RefCell<Vec<&'static str>>,
    }

    impl Fake {
        fn with(name: &str, data: &[u8]) -> Self {
            let fake = Self::default();
            fake.files.borrow_mut().insert(name.into(), data.into());
            fake
        }
        fn step(&self, what: &'static str) -> Result<(), Error> {
            self.calls.borrow_mut().push(what);
            let n = self.ops.get() + 1;
            self.ops.set(n);
            let at = self.fail_at.get();
            if at != 0 && (n == at || (self.cut.get() && n > at)) {
                return Err(Error::Io);
            }
            Ok(())
        }
        fn get(&self, name: &str) -> Option<Vec<u8>> {
            self.files.borrow().get(name).cloned()
        }
        /// Power is back: nothing fails any more.
        fn restart(&self) {
            self.fail_at.set(0);
            self.cut.set(false);
        }
    }

    impl Volume for Fake {
        fn size(&self, name: &str) -> Result<u64, Error> {
            self.step("size")?;
            self.get(name)
                .map(|f| f.len() as u64)
                .ok_or(Error::NotFound)
        }
        fn read_at(&self, name: &str, at: u64, out: &mut [u8]) -> Result<usize, Error> {
            self.step("read")?;
            let file = self.get(name).ok_or(Error::NotFound)?;
            let at = (at as usize).min(file.len());
            let mut n = out.len().min(file.len() - at);
            if self.short.get() != 0 {
                n = n.min(self.short.get());
            }
            out[..n].copy_from_slice(&file[at..at + n]);
            Ok(n)
        }
        fn write(&self, name: &str, data: &[u8]) -> Result<(), Error> {
            assert!(data.len() <= MAX_DATA, "a write is one call");
            self.step("write")?;
            self.files.borrow_mut().insert(name.into(), data.into());
            Ok(())
        }
        fn append(&self, name: &str, data: &[u8]) -> Result<(), Error> {
            assert!(data.len() <= MAX_DATA, "an append is one call");
            self.step("append")?;
            self.files
                .borrow_mut()
                .get_mut(name)
                .ok_or(Error::NotFound)?
                .extend_from_slice(data);
            Ok(())
        }
        fn rename(&self, from: &str, to: &str) -> Result<(), Error> {
            self.step("rename")?;
            let mut files = self.files.borrow_mut();
            let data = files.remove(from).ok_or(Error::NotFound)?;
            files.insert(to.into(), data);
            Ok(())
        }
        fn remove(&self, name: &str) -> Result<(), Error> {
            self.step("remove")?;
            self.files
                .borrow_mut()
                .remove(name)
                .map(|_| ())
                .ok_or(Error::NotFound)
        }
        fn sync(&self) -> Result<(), Error> {
            self.step("sync")
        }
    }

    fn bytes(n: usize, fill: u8) -> Vec<u8> {
        vec![fill; n]
    }

    #[test]
    fn four_kib_is_one_call_and_one_more_byte_goes_through_a_temp_file() {
        let old = bytes(10, b'o');
        let fake = Fake::with("f", &old);
        replace_on(&fake, "f", &bytes(MAX_DATA, b'a')).unwrap();
        // One write; the leftover temp is cleaned up.
        assert_eq!(*fake.calls.borrow(), ["write", "remove"]);
        assert_eq!(fake.get("f").unwrap(), bytes(MAX_DATA, b'a'));

        let fake = Fake::with("f", &old);
        replace_on(&fake, "f", &bytes(MAX_DATA + 1, b'b')).unwrap();
        assert_eq!(
            *fake.calls.borrow(),
            ["write", "append", "sync", "rename"],
            "first chunk, the rest, flush, then rename over the old"
        );
        assert_eq!(fake.get("f").unwrap(), bytes(MAX_DATA + 1, b'b'));
        assert!(fake.get("f.new").is_none());

        // Several parts.
        let fake = Fake::with("f", &old);
        let big: Vec<u8> = (0..MAX_DATA * 3 + 5).map(|i| i as u8).collect();
        replace_on(&fake, "f", &big).unwrap();
        assert_eq!(fake.get("f").unwrap(), big);
        assert_eq!(
            fake.calls
                .borrow()
                .iter()
                .filter(|c| **c == "append")
                .count(),
            3
        );
    }

    #[test]
    fn a_small_write_removes_a_temp_file_an_earlier_cut_left() {
        let fake = Fake::with("f", b"old");
        fake.files
            .borrow_mut()
            .insert("f.new".into(), bytes(9000, b'x'));
        replace_on(&fake, "f", b"new").unwrap();
        assert_eq!(fake.get("f").unwrap(), b"new");
        assert!(fake.get("f.new").is_none());
        // And one that is not there is no error.
        replace_on(&fake, "f", b"newer").unwrap();
    }

    #[test]
    fn a_refused_step_leaves_the_old_file_and_no_temp_file() {
        let old = bytes(100, b'o');
        let new = bytes(MAX_DATA * 2 + 1, b'n');
        let total = {
            let fake = Fake::with("f", &old);
            replace_on(&fake, "f", &new).unwrap();
            fake.ops.get()
        };
        for at in 1..=total {
            let fake = Fake::with("f", &old);
            fake.fail_at.set(at);
            assert!(replace_on(&fake, "f", &new).is_err(), "step {at}");
            assert_eq!(fake.get("f").unwrap(), old, "step {at}: the old file");
            assert!(fake.get("f.new").is_none(), "step {at}: no temp file");
            // A retry works.
            fake.restart();
            replace_on(&fake, "f", &new).unwrap();
            assert_eq!(fake.get("f").unwrap(), new);
        }
    }

    #[test]
    fn a_power_cut_at_any_step_leaves_the_old_file_or_the_new_whole() {
        let old = bytes(100, b'o');
        for size in [MAX_DATA, MAX_DATA + 1, MAX_DATA * 2 + 1] {
            let new = bytes(size, b'n');
            let total = {
                let fake = Fake::with("f", &old);
                replace_on(&fake, "f", &new).unwrap();
                fake.ops.get()
            };
            for at in 1..=total {
                let fake = Fake::with("f", &old);
                fake.fail_at.set(at);
                fake.cut.set(true);
                let _ = replace_on(&fake, "f", &new);
                let file = fake.get("f").unwrap();
                assert!(file == old || file == new, "size {size} cut {at}: whole");
                // After the restart the next save works and leaves no temp file.
                fake.restart();
                replace_on(&fake, "f", &new).unwrap();
                assert_eq!(fake.get("f").unwrap(), new);
                assert!(fake.get("f.new").is_none(), "size {size} cut {at}");
            }
        }
    }

    #[test]
    fn a_power_cut_in_the_appends_leaves_a_stale_temp_that_the_next_write_replaces() {
        let old = bytes(10, b'o');
        let fake = Fake::with("f", &old);
        // Cut at the second append: the temp holds two chunks of three.
        fake.fail_at.set(3);
        fake.cut.set(true);
        let new = bytes(MAX_DATA * 3, b'n');
        assert!(replace_on(&fake, "f", &new).is_err());
        assert_eq!(fake.get("f").unwrap(), old);
        assert_eq!(fake.get("f.new").unwrap().len(), MAX_DATA);
        fake.restart();
        // The first write of the next save replaces the stale temp.
        let next = bytes(MAX_DATA + 7, b'm');
        replace_on(&fake, "f", &next).unwrap();
        assert_eq!(fake.get("f").unwrap(), next);
        assert!(fake.get("f.new").is_none());
    }

    #[test]
    fn reads_at_the_limit_and_one_beyond() {
        for size in [0, 1, MAX_DATA - 1, MAX_DATA, MAX_DATA + 1, MAX_DATA * 2 + 3] {
            let data: Vec<u8> = (0..size).map(|i| (i * 7) as u8).collect();
            let fake = Fake::with("f", &data);
            assert_eq!(read_all_on(&fake, "f", size).unwrap().unwrap(), data);
            assert_eq!(read_all_on(&fake, "f", size + 1).unwrap().unwrap(), data);
            if size > 0 {
                assert_eq!(read_all_on(&fake, "f", size - 1), Err(Error::NoSpace));
            }
            // Short reads are put together.
            fake.short.set(1000);
            assert_eq!(read_all_on(&fake, "f", size).unwrap().unwrap(), data);
        }
        let fake = Fake::default();
        assert_eq!(read_all_on(&fake, "missing", 10), Ok(None));
    }

    #[test]
    fn a_read_that_fails_or_comes_up_empty_is_an_error_not_a_short_file() {
        let data = bytes(MAX_DATA * 2, b'd');
        let total = {
            let fake = Fake::with("f", &data);
            read_all_on(&fake, "f", usize::MAX).unwrap();
            fake.ops.get()
        };
        for at in 1..=total {
            let fake = Fake::with("f", &data);
            fake.fail_at.set(at);
            assert_eq!(read_all_on(&fake, "f", usize::MAX), Err(Error::Io));
        }
        // A file that shrinks under the read: the size says 2 chunks, the read gives 0.
        struct Shrinking(Fake);
        impl Volume for Shrinking {
            fn size(&self, name: &str) -> Result<u64, Error> {
                self.0.size(name)
            }
            fn read_at(&self, _: &str, _: u64, _: &mut [u8]) -> Result<usize, Error> {
                Ok(0)
            }
            fn write(&self, n: &str, d: &[u8]) -> Result<(), Error> {
                self.0.write(n, d)
            }
            fn append(&self, n: &str, d: &[u8]) -> Result<(), Error> {
                self.0.append(n, d)
            }
            fn rename(&self, a: &str, b: &str) -> Result<(), Error> {
                self.0.rename(a, b)
            }
            fn remove(&self, n: &str) -> Result<(), Error> {
                self.0.remove(n)
            }
            fn sync(&self) -> Result<(), Error> {
                self.0.sync()
            }
        }
        assert_eq!(
            read_all_on(&Shrinking(Fake::with("f", &data)), "f", usize::MAX),
            Err(Error::Io)
        );
    }

    #[test]
    fn the_head_of_a_file_is_what_is_there() {
        let fake = Fake::with("f", b"# format 2\nrest");
        assert_eq!(read_head_on(&fake, "f", 4).unwrap().unwrap(), b"# fo");
        assert_eq!(
            read_head_on(&fake, "f", 64).unwrap().unwrap(),
            b"# format 2\nrest"
        );
        assert_eq!(read_head_on(&fake, "g", 4), Ok(None));
    }

    /// Storage that holds nothing and allocates nothing: a file of `SIZE` zeros.
    struct Zeros;
    const SIZE: usize = MAX_DATA + 10;
    impl Volume for Zeros {
        fn size(&self, _: &str) -> Result<u64, Error> {
            Ok(SIZE as u64)
        }
        fn read_at(&self, _: &str, at: u64, out: &mut [u8]) -> Result<usize, Error> {
            let n = out.len().min(SIZE - at as usize);
            out[..n].fill(0);
            Ok(n)
        }
        fn write(&self, _: &str, _: &[u8]) -> Result<(), Error> {
            Ok(())
        }
        fn append(&self, _: &str, _: &[u8]) -> Result<(), Error> {
            Ok(())
        }
        fn rename(&self, _: &str, _: &str) -> Result<(), Error> {
            Ok(())
        }
        fn remove(&self, _: &str) -> Result<(), Error> {
            Ok(())
        }
        fn sync(&self) -> Result<(), Error> {
            Ok(())
        }
    }

    #[test]
    fn a_refused_allocation_is_no_memory_and_the_next_try_works() {
        crate::fail_alloc::sweep(|| {
            read_all_on(&Zeros, "f", usize::MAX).map(|r| r.map(|b| b.len()))
        });
        crate::fail_alloc::sweep(|| read_head_on(&Zeros, "f", 64).map(|r| r.map(|b| b.len())));
        let big = [1u8; MAX_DATA + 1];
        crate::fail_alloc::sweep(|| replace_on(&Zeros, "f", &big));
        crate::fail_alloc::sweep(|| replace_on(&Zeros, "f", b"small"));
    }
}
