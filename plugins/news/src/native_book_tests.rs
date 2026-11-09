//! A feed in a file made into the page of its book (`native_book`): the same
//! page and result as the interface-6 conversion, over a volume in memory, and
//! refusing instead of aborting.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use crate::fail_alloc::sweep;
use crate::native_book::{Failure, make_page};
use crate::native_convert_tests::{day_text, feeds, old};
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use paperpaw_sdk::native::{Error, storage::MAX_DATA};
use paperpaw_ui_kit::files::Volume;

/// The storage calls over files in memory.
#[derive(Default)]
struct Mem {
    files: RefCell<BTreeMap<String, Vec<u8>>>,
    /// Every append is at most one call's data.
    longest: RefCell<usize>,
}

impl Mem {
    fn with(name: &str, bytes: &[u8]) -> Self {
        let mem = Self::default();
        mem.files.borrow_mut().insert(name.into(), bytes.into());
        mem
    }

    fn get(&self, name: &str) -> Option<Vec<u8>> {
        self.files.borrow().get(name).cloned()
    }
}

impl Volume for Mem {
    fn size(&self, name: &str) -> Result<u64, Error> {
        self.files
            .borrow()
            .get(name)
            .map(|f| f.len() as u64)
            .ok_or(Error::NotFound)
    }

    fn read_at(&self, name: &str, at: u64, out: &mut [u8]) -> Result<usize, Error> {
        let files = self.files.borrow();
        let file = files.get(name).ok_or(Error::NotFound)?;
        let from = usize::try_from(at).unwrap().min(file.len());
        let n = out.len().min(file.len() - from);
        out[..n].copy_from_slice(&file[from..from + n]);
        Ok(n)
    }

    fn write(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        self.files.borrow_mut().insert(name.into(), data.into());
        Ok(())
    }

    fn append(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        assert!(data.len() <= MAX_DATA);
        let mut longest = self.longest.borrow_mut();
        *longest = (*longest).max(data.len());
        self.files
            .borrow_mut()
            .entry(name.into())
            .or_default()
            .extend_from_slice(data);
        Ok(())
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), Error> {
        let file = self.files.borrow_mut().remove(from).ok_or(Error::NotFound)?;
        self.files.borrow_mut().insert(to.into(), file);
        Ok(())
    }

    fn remove(&self, name: &str) -> Result<(), Error> {
        self.files
            .borrow_mut()
            .remove(name)
            .map(|_| ())
            .ok_or(Error::NotFound)
    }

    fn sync(&self) -> Result<(), Error> {
        Ok(())
    }
}

#[test]
fn a_feed_in_a_file_becomes_the_page_of_interface_6() {
    for feed in feeds() {
        for (articles, text) in [(10, 10_000), (5, 100), (1, 8 * 1024), (200, 64 * 1024)] {
            let (want, page) = old(feed.as_bytes(), articles, text);
            let mem = Mem::with("feed.tmp", feed.as_bytes());
            let got = make_page(&mem, ("feed.tmp", "page.tmp"), (articles, text), &day_text);
            match (want, got) {
                (Ok(w), Ok(g)) => {
                    assert_eq!((w.title, w.articles, w.headlines), (g.title, g.articles, g.headlines));
                    assert_eq!(page, mem.get("page.tmp").unwrap());
                }
                // No article: no page, and none left behind.
                (Ok(w), Err(Failure::Failed)) => {
                    assert_eq!(w.articles, 0);
                    assert!(mem.get("page.tmp").is_none());
                }
                (Err(_), Err(_)) => assert!(mem.get("page.tmp").is_none()),
                (want, got) => panic!("{want:?} against {got:?} for {feed}"),
            }
        }
    }
}

#[test]
fn a_page_of_an_earlier_try_is_no_start() {
    let feed = &feeds()[0];
    let mem = Mem::with("feed.tmp", feed.as_bytes());
    mem.write("page.tmp", b"old and half").unwrap();
    let got = make_page(&mem, ("feed.tmp", "page.tmp"), (10, 10_000), &day_text).unwrap();
    assert!(got.articles > 0);
    let page = mem.get("page.tmp").unwrap();
    assert!(page.starts_with(crate::native_convert::HEAD.as_bytes()));
    assert!(page.ends_with(crate::native_convert::TAIL.as_bytes()));
}

#[test]
fn a_long_page_is_written_in_calls_of_the_storage_size() {
    let feed = &feeds()[3];
    let mem = Mem::with("feed.tmp", feed.as_bytes());
    make_page(&mem, ("feed.tmp", "page.tmp"), (30, 64 * 1024), &day_text).unwrap();
    assert!(mem.get("page.tmp").unwrap().len() > MAX_DATA);
    assert!(*mem.longest.borrow() <= MAX_DATA);
}

#[test]
fn no_file_and_a_file_over_the_limit_are_no_page() {
    let mem = Mem::default();
    let got = make_page(&mem, ("feed.tmp", "page.tmp"), (10, 1000), &day_text);
    assert_eq!(got, Err(Failure::Failed));
    let big = Mem::with("feed.tmp", &alloc::vec![b' '; 4 * 1024 * 1024 + 1]);
    let got = make_page(&big, ("feed.tmp", "page.tmp"), (10, 1000), &day_text);
    assert_eq!(got, Err(Failure::Failed));
    assert!(big.get("page.tmp").is_none());
}

/// A volume that allocates nothing: it reads one feed and counts what is
/// written, so a sweep refuses only the plugin's own allocations.
struct Sink<'a> {
    feed: &'a [u8],
    written: core::cell::Cell<usize>,
}

impl Volume for Sink<'_> {
    fn size(&self, _: &str) -> Result<u64, Error> {
        Ok(self.feed.len() as u64)
    }

    fn read_at(&self, _: &str, at: u64, out: &mut [u8]) -> Result<usize, Error> {
        let from = usize::try_from(at).map_err(|_| Error::Invalid)?.min(self.feed.len());
        let n = out.len().min(self.feed.len() - from);
        out[..n].copy_from_slice(&self.feed[from..from + n]);
        Ok(n)
    }

    fn write(&self, _: &str, _: &[u8]) -> Result<(), Error> {
        Ok(())
    }

    fn append(&self, _: &str, data: &[u8]) -> Result<(), Error> {
        self.written.set(self.written.get() + data.len());
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
fn refused_memory_ends_in_memory_never_an_abort() {
    let feeds = feeds();
    let sink = Sink {
        feed: feeds[0].as_bytes(),
        written: core::cell::Cell::new(0),
    };
    sweep(|| {
        match make_page(&sink, ("feed.tmp", "page.tmp"), (10, 10_000), &day_text) {
            Ok(_) => Ok(()),
            Err(Failure::Memory) => Err(Error::NoMemory),
            Err(Failure::Failed) => Err(Error::Invalid),
        }
        // A converted feed that failed for another reason is no refusal.
    });
}
