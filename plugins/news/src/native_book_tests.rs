//! A feed in a file made into the page of its book (`native_book`): the same
//! page and result as the interface-6 conversion pinned in `testdata`, over a volume in memory, and
//! refusing instead of aborting.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use crate::fail_alloc::sweep;
use crate::native_book::{Failure, make_page};
use crate::native_convert_tests::{day_text, feeds, pinned, pinned_line};
use crate::test_volume::Mem;
use paperpaw_sdk::native::{Error, storage::MAX_DATA};
use paperpaw_ui_kit::files::Volume;

#[test]
fn a_feed_in_a_file_becomes_the_page_of_interface_6() {
    for (index, feed) in feeds().iter().enumerate() {
        for (articles, text) in [(10, 10_000), (5, 100), (1, 8 * 1024), (200, 64 * 1024)] {
            let want = pinned(index, articles, text);
            let mem = Mem::with("feed.tmp", feed.as_bytes());
            let got = make_page(&mem, ("feed.tmp", "page.tmp"), (articles, text), &day_text);
            match got {
                Ok(g) => {
                    let page = mem.get("page.tmp").unwrap();
                    assert_eq!(pinned_line(&g.title, g.articles, &g.headlines, &page), want);
                }
                // No article: no page, and none left behind.
                Err(Failure::Failed) => {
                    assert!(want.contains(" 0 [] page="), "{want} for {feed:.60}");
                    assert!(mem.get("page.tmp").is_none());
                }
                got => panic!("{got:?} for {feed}"),
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

/// A volume whose power is cut after `left` appends: from then on every
/// call that changes a file fails and nothing is cleaned up.
struct Cut<'a> {
    mem: &'a Mem,
    left: core::cell::Cell<usize>,
}

impl Volume for Cut<'_> {
    fn size(&self, name: &str) -> Result<u64, Error> {
        self.mem.size(name)
    }

    fn read_at(&self, name: &str, at: u64, out: &mut [u8]) -> Result<usize, Error> {
        self.mem.read_at(name, at, out)
    }

    fn write(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        self.mem.write(name, data)
    }

    fn append(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        let left = self.left.get();
        if left == 0 {
            return Err(Error::Invalid);
        }
        self.left.set(left - 1);
        self.mem.append(name, data)
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), Error> {
        self.mem.rename(from, to)
    }

    fn remove(&self, name: &str) -> Result<(), Error> {
        if self.left.get() == 0 {
            return Err(Error::Invalid);
        }
        self.mem.remove(name)
    }

    fn sync(&self) -> Result<(), Error> {
        self.mem.sync()
    }
}

#[test]
fn a_cut_leaves_a_stale_page_the_next_call_replaces() {
    let feed = &feeds()[3];
    // How many appends a whole page takes.
    let mem = Mem::with("feed.tmp", feed.as_bytes());
    let count = Cut {
        mem: &mem,
        left: core::cell::Cell::new(usize::MAX),
    };
    make_page(&count, ("feed.tmp", "page.tmp"), (30, 64 * 1024), &day_text).unwrap();
    let whole = mem.get("page.tmp").unwrap();
    let appends = usize::MAX - count.left.get();
    assert!(appends > 2);
    for left in 1..appends {
        let mem = Mem::with("feed.tmp", feed.as_bytes());
        let cut = Cut {
            mem: &mem,
            left: core::cell::Cell::new(left),
        };
        let got = make_page(&cut, ("feed.tmp", "page.tmp"), (30, 64 * 1024), &day_text);
        // The caller imports only after Ok: a cut is never Ok.
        assert_eq!(got, Err(Failure::Failed), "cut after {left}");
        let stale = mem.get("page.tmp").unwrap();
        assert!(stale.len() < whole.len(), "cut after {left}");
        // The next call starts afresh and makes the whole page.
        make_page(&mem, ("feed.tmp", "page.tmp"), (30, 64 * 1024), &day_text).unwrap();
        assert_eq!(mem.get("page.tmp").unwrap(), whole);
    }
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
        let from = usize::try_from(at)
            .map_err(|_| Error::Invalid)?
            .min(self.feed.len());
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
