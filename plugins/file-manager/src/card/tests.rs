//! The translation between the card's calls and `Storage`, over a fake card
//! that lists three entries a page and says what the host says.
#![allow(clippy::unwrap_used, reason = "test assertions")]

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::string::{String, ToString};
use std::vec::Vec;

use paperpaw_sdk::native::Error;
use paperpaw_sdk::{Storage, StorageError};

use super::{Card, CardStorage, Meta, Visit};

const PAGE: usize = 3;

/// Folders are paths with `None`; files hold bytes.
#[derive(Default)]
pub(crate) struct Fake {
    nodes: RefCell<BTreeMap<String, Option<Vec<u8>>>>,
    /// Calls of `list`, to see the pages.
    pages: Cell<usize>,
    case: bool,
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
fn a_move_replaces_a_file_but_never_a_folder() {
    let card = CardStorage(Fake::with(&[("a", "new"), ("b", "old")], &["f"]));
    card.rename("a", "b").unwrap();
    assert_eq!(card.size("b"), Ok(3));
    let card = CardStorage(Fake::with(&[("a", "1")], &["f"]));
    assert_eq!(card.rename("a", "f"), Err(StorageError::InvalidPath));
    assert_eq!(card.size("a"), Ok(1), "the file stays");
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
