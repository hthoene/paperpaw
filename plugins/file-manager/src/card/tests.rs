//! The translation between the card's calls and `Storage`, over a fake card
//! that lists three entries a page and says what the host says.
#![allow(clippy::unwrap_used, reason = "test assertions")]

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::string::{String, ToString};
use std::vec::Vec;

use paperpaw_sdk::native::Error;
use paperpaw_sdk::{Storage, StorageError};

use super::{Card, CardStorage, Meta, Visit, Watch, take_buffers};
use crate::model::testing::{fixture, sweep_on};

const PAGE: usize = 3;

/// Folders are paths with `None`; files hold bytes.
#[derive(Default)]
pub(crate) struct Fake {
    nodes: RefCell<BTreeMap<String, Option<Vec<u8>>>>,
    /// Calls of `list`, to see the pages.
    pages: Cell<usize>,
    case: bool,
    /// What `stat` says instead of the truth, for a read error.
    stat_fails: Cell<Option<Error>>,
    /// Calls of `append` still taken; the next one fails.
    appends_left: Cell<Option<usize>>,
    /// What `list` says instead of the entries.
    list_fails: Cell<Option<Error>>,
}

impl Fake {
    pub(crate) fn with(files: &[(&str, &str)], folders: &[&str]) -> Self {
        let fake = Self::default();
        for folder in folders {
            fake.nodes.borrow_mut().insert((*folder).to_string(), None);
        }
        for (path, data) in files {
            fake.nodes
                .borrow_mut()
                .insert((*path).to_string(), Some(data.as_bytes().to_vec()));
        }
        fake
    }

    /// The bytes of the file there.
    pub(crate) fn data(&self, path: &str) -> Option<Vec<u8>> {
        self.nodes.borrow().get(path).cloned().flatten()
    }

    fn parent_exists(&self, path: &str) -> bool {
        match path.rsplit_once('/') {
            None => true,
            Some((parent, _)) => matches!(self.nodes.borrow().get(parent), Some(None)),
        }
    }

    fn meta(node: &Option<Vec<u8>>) -> Meta {
        Meta {
            is_dir: node.is_none(),
            size: node.as_ref().map_or(0, |d| d.len() as u64),
            modified: Some(1_000),
        }
    }
}

impl Card for Fake {
    fn list(&self, dir: &str, cursor: u64, visit: &mut Visit<'_>) -> Result<Option<u64>, Error> {
        self.pages.set(self.pages.get() + 1);
        if let Some(error) = self.list_fails.get() {
            return Err(error);
        }
        let nodes = self.nodes.borrow();
        if !dir.is_empty() && !matches!(nodes.get(dir), Some(None)) {
            return Err(Error::NotFound);
        }
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            std::format!("{dir}/")
        };
        let inside: Vec<_> = nodes
            .iter()
            .filter(|(path, _)| {
                path.strip_prefix(&prefix)
                    .is_some_and(|rest| !rest.is_empty() && !rest.contains('/'))
            })
            .collect();
        let from = cursor as usize;
        for (path, node) in inside.iter().skip(from).take(PAGE) {
            visit(&path[prefix.len()..], Self::meta(node));
        }
        Ok((from + PAGE < inside.len()).then(|| (from + PAGE) as u64))
    }

    fn stat(&self, path: &str) -> Result<Meta, Error> {
        if let Some(error) = self.stat_fails.get() {
            return Err(error);
        }
        self.nodes
            .borrow()
            .get(path)
            .map(Self::meta)
            .ok_or(Error::NotFound)
    }

    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, Error> {
        let nodes = self.nodes.borrow();
        let Some(Some(data)) = nodes.get(path) else {
            return Err(Error::NotFound);
        };
        let from = (offset as usize).min(data.len());
        let n = buf.len().min(data.len() - from);
        buf[..n].copy_from_slice(&data[from..from + n]);
        Ok(n)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), Error> {
        self.nodes
            .borrow_mut()
            .insert(path.to_string(), Some(data.to_vec()));
        Ok(())
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), Error> {
        if let Some(left) = self.appends_left.get() {
            if left == 0 {
                return Err(Error::Io);
            }
            self.appends_left.set(Some(left - 1));
        }
        let mut nodes = self.nodes.borrow_mut();
        let node = nodes.entry(path.to_string()).or_insert(Some(Vec::new()));
        node.as_mut().ok_or(Error::Invalid)?.extend_from_slice(data);
        Ok(())
    }

    fn remove(&self, path: &str) -> Result<(), Error> {
        self.nodes
            .borrow_mut()
            .remove(path)
            .map(|_| ())
            .ok_or(Error::NotFound)
    }

    fn make_dir(&self, path: &str) -> Result<(), Error> {
        let mut at = String::new();
        for part in path.split('/') {
            if !at.is_empty() {
                at.push('/');
            }
            at.push_str(part);
            let mut nodes = self.nodes.borrow_mut();
            match nodes.get(&at) {
                Some(Some(_)) => return Err(Error::Invalid),
                Some(None) => {}
                None => {
                    nodes.insert(at.clone(), None);
                }
            }
        }
        Ok(())
    }

    fn remove_tree(&self, path: &str) -> Result<(), Error> {
        let prefix = std::format!("{path}/");
        let mut nodes = self.nodes.borrow_mut();
        if !nodes.contains_key(path) {
            return Err(Error::NotFound);
        }
        nodes.retain(|p, _| p != path && !p.starts_with(&prefix));
        Ok(())
    }

    fn move_entry(&self, from: &str, to: &str, replace: bool) -> Result<(), Error> {
        if !self.nodes.borrow().contains_key(from) || !self.parent_exists(to) {
            return Err(Error::NotFound);
        }
        let mut nodes = self.nodes.borrow_mut();
        match nodes.get(to) {
            Some(None) => return Err(Error::Invalid),
            Some(Some(_)) if !replace => return Err(Error::State),
            _ => {}
        }
        let prefix = std::format!("{from}/");
        let moved: Vec<_> = nodes
            .iter()
            .filter(|(p, _)| *p == from || p.starts_with(&prefix))
            .map(|(p, n)| (p.clone(), n.clone()))
            .collect();
        for (path, node) in moved {
            nodes.remove(&path);
            nodes.insert(std::format!("{to}{}", &path[from.len()..]), node);
        }
        Ok(())
    }

    fn sync(&self) -> Result<(), Error> {
        Ok(())
    }

    fn ignores_case(&self) -> bool {
        self.case
    }
}

fn names(storage: &CardStorage<Fake>, dir: &str) -> Vec<String> {
    let mut out = Vec::new();
    storage
        .list(dir, &mut |e| out.push(e.name.to_string()))
        .unwrap();
    out.sort();
    out
}

#[test]
fn a_folder_longer_than_a_page_is_listed_whole() {
    let files: Vec<_> = (0..8).map(|n| std::format!("d/f{n}")).collect();
    let fake = Fake::with(&[], &["d"]);
    for f in &files {
        fake.write(f, b"x").unwrap();
    }
    let card = CardStorage(fake);
    assert_eq!(names(&card, "d").len(), 8);
    assert_eq!(card.0.pages.get(), 3, "three pages of three");
    // The top of the card lists too.
    assert_eq!(names(&card, ""), ["d"]);
    assert_eq!(card.list("nope", &mut |_| {}), Err(StorageError::NotFound));
}

#[test]
fn a_folder_has_no_size_and_a_file_has() {
    let card = CardStorage(Fake::with(&[("d/a.txt", "hello")], &["d"]));
    assert_eq!(card.size("d/a.txt"), Ok(5));
    assert_eq!(card.size("d"), Err(StorageError::NotFound));
    assert_eq!(card.size("missing"), Err(StorageError::NotFound));
    assert!(card.exists("d/a.txt") && !card.exists("d"));
    assert_eq!(card.modified("d/a.txt"), Some(1_000));
}

#[test]
fn reads_and_writes_go_in_pieces_the_host_takes() {
    let card = CardStorage(Fake::default());
    let big = std::vec![7u8; 10_000];
    card.write("a", b"").unwrap();
    card.append("a", &big).unwrap();
    assert_eq!(card.size("a"), Ok(10_000));
    let mut back = std::vec![0u8; 10_000];
    assert_eq!(card.read_at("a", 0, &mut back), Ok(10_000));
    assert_eq!(back, big);
    // Past the end is the end.
    assert_eq!(card.read_at("a", 20_000, &mut back), Ok(0));
    // A whole write of more than a piece is refused, not cut.
    assert_eq!(card.write("b", &big), Err(StorageError::TooLarge));
    assert!(!card.exists("b"));
}

#[test]
fn removing_a_missing_file_is_not_an_error() {
    let card = CardStorage(Fake::with(&[("a", "1")], &[]));
    assert_eq!(card.remove("a"), Ok(()));
    assert_eq!(card.remove("a"), Ok(()));
    assert_eq!(card.remove_dir_all("gone"), Ok(()));
}

#[test]
fn a_folder_is_removed_with_what_is_in_it() {
    let card = CardStorage(Fake::with(&[("d/a", "1"), ("d/e/b", "2")], &["d", "d/e"]));
    card.remove_dir_all("d").unwrap();
    assert!(names(&card, "").is_empty());
}

#[test]
fn a_move_makes_the_folder_it_goes_into() {
    let card = CardStorage(Fake::with(&[("a.txt", "1")], &[]));
    card.rename("a.txt", "new/deep/a.txt").unwrap();
    assert_eq!(card.size("new/deep/a.txt"), Ok(1));
    assert!(!card.exists("a.txt"));
    // A missing source is a missing source: no folder is made for it.
    assert_eq!(
        card.rename("gone.txt", "other/gone.txt"),
        Err(StorageError::NotFound)
    );
    assert_eq!(names(&card, ""), ["new"]);
}

#[test]
fn a_move_replaces_nothing_neither_a_file_nor_a_folder() {
    let card = CardStorage(Fake::with(&[("a", "new"), ("b", "old")], &["f"]));
    assert_eq!(card.rename("a", "b"), Err(StorageError::Io));
    assert_eq!(card.size("a"), Ok(3), "the file stays");
    let mut back = [0u8; 3];
    card.read_at("b", 0, &mut back).unwrap();
    assert_eq!(&back, b"old", "the other is as it was");
    let card = CardStorage(Fake::with(&[("a", "1")], &["f"]));
    assert_eq!(card.rename("a", "f"), Err(StorageError::InvalidPath));
    assert_eq!(card.size("a"), Ok(1), "the file stays");
    // Also where the folder above has to be made first.
    let card = CardStorage(Fake::with(&[("a", "1"), ("d/a", "2")], &["d"]));
    assert_eq!(card.rename("a", "d/a"), Err(StorageError::Io));
    assert_eq!(card.size("d/a"), Ok(1), "the file there stays");
}

#[test]
fn a_card_that_cannot_say_has_the_file() {
    let fake = Fake::with(&[("a", "1")], &["d"]);
    let card = CardStorage(fake);
    assert!(card.exists("a") && !card.exists("missing") && !card.exists("d"));
    // A read error is not "not there": nothing is made over a name then.
    card.0.stat_fails.set(Some(Error::Io));
    assert!(card.exists("missing"), "an error is not a free name");
    assert!(card.exists("a"));
    card.0.stat_fails.set(Some(Error::NotFound));
    assert!(!card.exists("a"), "only not found says free");
    // The model picks a name the card could not vouch for never.
    card.0.stat_fails.set(Some(Error::Io));
    let free = crate::model::path::free_path(&card, "", "new.txt");
    assert!(free.as_ref().is_ok_and(|p| p != "new.txt"), "{free:?}");
}

#[test]
fn a_copy_that_fails_halfway_leaves_the_target_free_and_the_source_whole() {
    let big = "x".repeat(20_000);
    for left in 0..5 {
        let card = CardStorage(Fake::with(&[("a.bin", &big)], &[]));
        card.0.appends_left.set(Some(left));
        let result = crate::model::ops::copy(&card, "a.bin", "b.bin");
        assert!(result.is_err(), "append {left} fails");
        card.0.appends_left.set(None);
        assert!(!card.exists("b.bin"), "no half copy under its name");
        assert!(!card.exists("b.bin.part"), "no part left");
        assert_eq!(card.size("a.bin"), Ok(20_000), "the source is whole");
    }
    let card = CardStorage(Fake::with(&[("a.bin", &big)], &[]));
    crate::model::ops::copy(&card, "a.bin", "b.bin").unwrap();
    assert_eq!(card.size("b.bin"), Ok(20_000));
}

#[test]
fn a_folder_moves_with_what_is_in_it() {
    let card = CardStorage(Fake::with(&[("d/a", "1")], &["d"]));
    card.rename("d", "e").unwrap();
    assert_eq!(card.size("e/a"), Ok(1));
}

#[test]
fn the_card_says_whether_names_differ_in_case() {
    let mut fake = Fake::default();
    assert!(!CardStorage(Fake::default()).ignores_case());
    fake.case = true;
    assert!(CardStorage(fake).ignores_case());
}

#[test]
fn host_errors_are_told_as_storage_errors() {
    use super::storage_error;
    assert_eq!(storage_error(Error::NotFound), StorageError::NotFound);
    assert_eq!(storage_error(Error::Full), StorageError::Full);
    assert_eq!(storage_error(Error::NoSpace), StorageError::TooLarge);
    assert_eq!(storage_error(Error::Denied), StorageError::Unsupported);
    assert_eq!(storage_error(Error::Invalid), StorageError::InvalidPath);
    for other in [Error::Io, Error::NoMemory, Error::Busy, Error::State] {
        assert_eq!(storage_error(other), StorageError::Io);
    }
}

#[test]
fn a_call_while_another_uses_the_buffers_is_busy_not_a_panic() {
    let (input, output) = (RefCell::new(Vec::new()), RefCell::new(Vec::new()));
    {
        let _held = take_buffers(&input, &output).unwrap();
        assert!(matches!(take_buffers(&input, &output), Err(Error::Busy)));
    }
    // One of the two held is as busy.
    let _one = output.borrow_mut();
    assert!(matches!(take_buffers(&input, &output), Err(Error::Busy)));
    drop(_one);
    assert!(take_buffers(&input, &output).is_ok(), "free again");
}

#[test]
fn the_host_running_out_of_memory_is_noted_and_reaches_the_reader() {
    use crate::browser::{Browser, Note};
    let watch = Watch::new(Fake::with(&[("a.txt", "1")], &[]));
    let card = CardStorage(watch);
    let mut browser = Browser::new(String::new(), false, 10);
    browser.reload(&card, None);
    assert!(!card.0.ran_out_of_memory());
    assert_eq!(browser.note, None);
    card.0.inner.list_fails.set(Some(Error::NoMemory));
    browser.reload(&card, None);
    // The storage's word for it is a general failure ...
    assert_eq!(browser.note, Some(Note::Failed(crate::model::Error::Io)));
    assert!(card.0.ran_out_of_memory());
    // ... which the flag makes into "not enough memory".
    browser.lacked_memory();
    assert_eq!(
        browser.note,
        Some(Note::Failed(crate::model::Error::NoMemory))
    );
    // Another failure stays what it was.
    card.0.inner.list_fails.set(Some(Error::NotFound));
    browser.reload(&card, None);
    browser.lacked_memory();
    assert_eq!(
        browser.note,
        Some(Note::Failed(crate::model::Error::NotFound))
    );
}

/// The fake card whose own allocations are not the plugin's (they are out of
/// the count and never refused).
pub(crate) struct QuietCard(pub(crate) Fake);

impl Card for QuietCard {
    fn list(&self, dir: &str, cursor: u64, visit: &mut Visit<'_>) -> Result<Option<u64>, Error> {
        let mut seen: Vec<(String, Meta)> = Vec::new();
        let next = fixture(|| {
            self.0.list(dir, cursor, &mut |name, meta| {
                seen.push((name.to_string(), meta));
            })
        })?;
        for (name, meta) in &seen {
            visit(name, *meta);
        }
        Ok(next)
    }
    fn stat(&self, path: &str) -> Result<Meta, Error> {
        fixture(|| self.0.stat(path))
    }
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, Error> {
        fixture(|| self.0.read_at(path, offset, buf))
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), Error> {
        fixture(|| self.0.write(path, data))
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), Error> {
        fixture(|| self.0.append(path, data))
    }
    fn remove(&self, path: &str) -> Result<(), Error> {
        fixture(|| self.0.remove(path))
    }
    fn make_dir(&self, path: &str) -> Result<(), Error> {
        fixture(|| self.0.make_dir(path))
    }
    fn remove_tree(&self, path: &str) -> Result<(), Error> {
        fixture(|| self.0.remove_tree(path))
    }
    fn move_entry(&self, from: &str, to: &str, replace: bool) -> Result<(), Error> {
        fixture(|| self.0.move_entry(from, to, replace))
    }
    fn sync(&self) -> Result<(), Error> {
        Ok(())
    }
    fn ignores_case(&self) -> bool {
        self.0.ignores_case()
    }
}

pub(crate) fn quiet_card() -> CardStorage<QuietCard> {
    let big = "x".repeat(9_000);
    CardStorage(QuietCard(fixture(|| {
        Fake::with(
            &[
                ("src/a.txt", "alpha"),
                ("src/big.bin", &big),
                ("src/sub/b.txt", "beta"),
                ("keep/old.txt", "old"),
            ],
            &["src", "src/sub", "keep"],
        )
    })))
}

/// No part is left, the source is whole, and a file at the target is whole or
/// not there.
fn card_is_intact(card: &CardStorage<QuietCard>) {
    let fake = &card.0.0;
    fixture(|| {
        let big = "x".repeat(9_000).into_bytes();
        assert!(!fake.nodes.borrow().keys().any(|k| k.ends_with(".part")));
        assert_eq!(fake.data("keep/old.txt").unwrap(), b"old");
        let moved = fake.data("dst/big.bin");
        let source = fake.data("src/big.bin");
        assert!(
            source.as_ref() == Some(&big) || (source.is_none() && moved.as_ref() == Some(&big)),
            "one whole copy of the file is there"
        );
        if let Some(got) = &moved {
            assert_eq!(got, &big, "a half file looks whole");
        }
    });
}

#[test]
fn a_refused_allocation_on_the_card_leaves_the_files_as_they_were() {
    use crate::model::ops::{copy, rename};
    sweep_on(
        "copy a tree",
        quiet_card,
        |card| copy(card, "src", "dst"),
        card_is_intact,
    );
    sweep_on(
        "rename",
        quiet_card,
        |card| rename(card, "src/a.txt", "z.txt").map(|_| ()),
        |card| {
            let fake = &card.0.0;
            fixture(|| {
                let (old, new) = (fake.data("src/a.txt"), fake.data("src/z.txt"));
                assert!(old.is_some() != new.is_some(), "the file is in one place");
            });
        },
    );
}
