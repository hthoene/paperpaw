//! The lists the plugin keeps (`native_kept`) against interface 6's (`kept`):
//! the same lines in the same files, read back as written, the files of
//! version 1 still read, a failure leaving the list as it was, and no more
//! than a few paths held at a time.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use crate::native_kept::{self as native, MOST_WAITING, Sent};
use crate::test_volume::Mem;
use alloc::format;
use alloc::string::{String, ToString};
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::files::Volume;

fn note(id: &str, text: &str) -> Sent {
    Sent {
        id: id.to_string(),
        text: text.to_string(),
    }
}

/// What version 1 wrote stays readable (see `docs/data-files.md`).
#[test]
fn files_of_version_1_are_read() {
    let new = Mem::default();
    new.write(native::SENT, include_bytes!("../testdata/sent.txt.v1"))
        .unwrap();
    new.write(
        native::WAITING,
        include_bytes!("../testdata/waiting.txt.v1"),
    )
    .unwrap();
    new.write(
        native::NOTEBOOK,
        include_bytes!("../testdata/notebook.txt.v1"),
    )
    .unwrap();
    assert_eq!(
        native::sent(&new, "Notes/Lewis Carroll - Alice.md").unwrap(),
        Some(note(
            "1ae79351dd744c2794eebb334b1629a7",
            "9e107d9d372bb6826bd81d3542a419d6"
        ))
    );
    assert_eq!(
        native::waiting(&new).unwrap(),
        ["Notes/Jane Austen - Emma.md"]
    );
    assert_eq!(
        native::notebook(&new).unwrap(),
        Some((
            String::from("PaperPaw"),
            String::from("7db793874c624138b3b4520ae5cdf20c")
        ))
    );
}

#[test]
fn a_line_with_other_fields_than_two_is_no_note() {
    let new = Mem::default();
    new.write(native::SENT, b"a.md\t1\nb.md\t1\t2\t3\nc.md\t1\t2\n")
        .unwrap();
    assert_eq!(native::sent(&new, "a.md").unwrap(), None);
    assert_eq!(native::sent(&new, "b.md").unwrap(), None);
    assert_eq!(native::sent(&new, "c.md").unwrap(), Some(note("1", "2")));
}

#[test]
fn a_long_list_is_taken_a_few_at_a_time_and_none_is_lost() {
    let new = Mem::default();
    for i in 0..40 {
        native::add_waiting(&new, &format!("Notes/{i}.md")).unwrap();
    }
    let first = native::waiting(&new).unwrap();
    assert_eq!(first.len(), MOST_WAITING);
    assert_eq!(first[0], "Notes/0.md");
    for path in &first {
        native::remove_waiting(&new, path).unwrap();
    }
    let rest = native::waiting(&new).unwrap();
    assert_eq!(rest.len(), 8);
    assert_eq!(rest[0], "Notes/32.md");
}

/// A volume whose power goes after `left` calls that change a file.
struct Cut<'a> {
    mem: &'a Mem,
    left: core::cell::Cell<usize>,
}

impl Cut<'_> {
    fn step(&self) -> Result<(), Error> {
        let left = self.left.get();
        if left == 0 {
            return Err(Error::Io);
        }
        self.left.set(left - 1);
        Ok(())
    }
}

impl Volume for Cut<'_> {
    fn size(&self, n: &str) -> Result<u64, Error> {
        self.mem.size(n)
    }
    fn read_at(&self, n: &str, at: u64, out: &mut [u8]) -> Result<usize, Error> {
        self.mem.read_at(n, at, out)
    }
    fn write(&self, n: &str, d: &[u8]) -> Result<(), Error> {
        self.step()?;
        self.mem.write(n, d)
    }
    fn append(&self, n: &str, d: &[u8]) -> Result<(), Error> {
        self.step()?;
        self.mem.append(n, d)
    }
    fn rename(&self, a: &str, b: &str) -> Result<(), Error> {
        self.step()?;
        self.mem.rename(a, b)
    }
    fn remove(&self, n: &str) -> Result<(), Error> {
        self.step()?;
        self.mem.remove(n)
    }
    fn sync(&self) -> Result<(), Error> {
        Ok(())
    }
}

#[test]
fn a_list_that_cannot_be_written_is_left_as_it_was() {
    let mem = Mem::default();
    native::add_waiting(&mem, "a.md").unwrap();
    native::add_waiting(&mem, "b.md").unwrap();
    for left in 0..6 {
        let cut = Cut {
            mem: &mem,
            left: core::cell::Cell::new(left),
        };
        let result = native::add_waiting(&cut, "c.md");
        if result.is_err() {
            assert_eq!(
                mem.get(native::WAITING).unwrap(),
                b"a.md\nb.md\n",
                "cut after {left}"
            );
        } else {
            break;
        }
    }
    native::add_waiting(&mem, "c.md").unwrap();
    assert_eq!(native::waiting(&mem).unwrap(), ["a.md", "b.md", "c.md"]);
}

#[test]
fn a_long_path_is_written_in_calls_the_storage_takes() {
    let mem = Mem::default();
    let long = "x".repeat(5000);
    native::add_waiting(&mem, &long).unwrap();
    native::add_waiting(&mem, "a.md").unwrap();
    assert!(*mem.longest.borrow() <= paperpaw_sdk::native::storage::MAX_DATA);
    // A path is cut at a line's longest when read back.
    assert_eq!(
        native::waiting(&mem).unwrap(),
        [long[..4096].to_string(), "a.md".to_string()]
    );
}
