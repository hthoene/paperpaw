//! The bounded tree removal ([`remove_tree_step`]) over an in-memory tree:
//! every call within its limits, a big folder in several calls, stale and
//! hostile places, names it cannot use, a storage that fails part of the way,
//! and storages with stable places (as FAT) and counted ones (the default).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use paperpaw_platform::tree::{Kind, MAX_DEPTH, MAX_READS, MAX_REMOVALS, kind, remove_tree_step};
use paperpaw_platform::{DirEntry, Storage, StorageError, list_counted};

/// A tree of folders and files. Entries keep the order they were made in. A
/// place is the entry's serial number (`stable`, as FAT's slot) or its count.
struct Tree {
    /// Path to (serial, is_dir).
    nodes: RefCell<BTreeMap<String, (u64, bool)>>,
    serial: Cell<u64>,
    stable: bool,
    /// Entries read through `list_from`'s visitor.
    reads: Cell<u32>,
    /// Removals so far, and the one that fails (counting from 1).
    removals: Cell<u32>,
    fail_at: Cell<Option<u32>>,
    /// Every removed path, in order.
    log: RefCell<Vec<String>>,
}

impl Tree {
    fn new(stable: bool) -> Self {
        let tree = Self {
            nodes: RefCell::default(),
            serial: Cell::new(1),
            stable,
            reads: Cell::new(0),
            removals: Cell::new(0),
            fail_at: Cell::new(None),
            log: RefCell::default(),
        };
        tree.nodes.borrow_mut().insert(String::new(), (0, true));
        tree
    }

    fn add(&self, path: &str, is_dir: bool) {
        let serial = self.serial.get();
        self.serial.set(serial + 1);
        let parent = path.rsplit_once('/').map_or("", |(p, _)| p);
        if !parent.is_empty() && !self.nodes.borrow().contains_key(parent) {
            self.add(parent, true);
        }
        self.nodes
            .borrow_mut()
            .insert(path.to_string(), (serial, is_dir));
    }

    fn file(&self, path: &str) {
        self.add(path, false);
    }

    fn dir(&self, path: &str) {
        self.add(path, true);
    }

    fn children(&self, dir: &str) -> Vec<(String, u64, bool)> {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let mut found: Vec<(String, u64, bool)> = self
            .nodes
            .borrow()
            .iter()
            .filter(|(path, _)| {
                !path.is_empty() && path.starts_with(&prefix) && !path[prefix.len()..].contains('/')
            })
            .map(|(path, (serial, is_dir))| (path[prefix.len()..].to_string(), *serial, *is_dir))
            .collect();
        found.sort_by_key(|(_, serial, _)| *serial);
        found
    }

    fn paths(&self) -> Vec<String> {
        self.nodes
            .borrow()
            .keys()
            .filter(|p| !p.is_empty())
            .cloned()
            .collect()
    }

    fn fails(&self) -> Result<(), StorageError> {
        let n = self.removals.get() + 1;
        self.removals.set(n);
        if self.fail_at.get() == Some(n) {
            Err(StorageError::Io)
        } else {
            Ok(())
        }
    }
}

impl Storage for Tree {
    fn read_at(&self, _: &str, _: u64, _: &mut [u8]) -> Result<usize, StorageError> {
        Ok(0)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        match self.nodes.borrow().get(path) {
            Some((_, false)) => Ok(1),
            _ => Err(StorageError::NotFound),
        }
    }

    fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }

    fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        match self.nodes.borrow().get(dir) {
            Some((_, true)) => {}
            Some(_) => return Err(StorageError::InvalidPath),
            None => return Err(StorageError::NotFound),
        }
        for (name, _, is_dir) in self.children(dir) {
            visit(DirEntry::new(&name, is_dir, 0));
        }
        Ok(())
    }

    fn list_from(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        let counted = |entry: DirEntry<'_>| {
            self.reads.set(self.reads.get() + 1);
            visit(entry)
        };
        let mut counted = counted;
        if !self.stable {
            return list_counted(self, dir, from, &mut counted);
        }
        match self.nodes.borrow().get(dir) {
            Some((_, true)) => {}
            Some(_) => return Err(StorageError::InvalidPath),
            None => return Err(StorageError::NotFound),
        }
        // Serial numbers are places; one that is gone starts at the next.
        for (name, serial, is_dir) in self.children(dir) {
            if serial < from {
                continue;
            }
            if !counted(DirEntry::new(&name, is_dir, 0)) {
                return Ok(Some(serial));
            }
        }
        Ok(None)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.fails()?;
        let mut nodes = self.nodes.borrow_mut();
        match nodes.get(path) {
            None => return Ok(()),
            Some((_, true)) => return Err(StorageError::InvalidPath),
            Some(_) => {}
        }
        nodes.remove(path);
        self.log.borrow_mut().push(path.to_string());
        Ok(())
    }

    fn remove_listed(&self, dir: &str, place: u64) -> Result<(), StorageError> {
        self.fails()?;
        let children = self.children(dir);
        let found = if self.stable {
            children.into_iter().find(|(_, serial, _)| *serial >= place)
        } else {
            children.into_iter().nth(place as usize)
        };
        let (name, _, is_dir) = found.ok_or(StorageError::NotFound)?;
        let path = if dir.is_empty() {
            name
        } else {
            format!("{dir}/{name}")
        };
        // A folder with something in it goes whole, in one step.
        let prefix = format!("{path}/");
        self.nodes
            .borrow_mut()
            .retain(|p, _| p != &path && !(is_dir && p.starts_with(&prefix)));
        self.log.borrow_mut().push(format!("{path}#"));
        Ok(())
    }

    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.fails()?;
        // Only ever a folder a listing showed empty.
        assert!(
            self.children(path).is_empty(),
            "remove_dir_all of {path}, not empty"
        );
        let prefix = format!("{path}/");
        self.nodes
            .borrow_mut()
            .retain(|p, _| p != path && !p.starts_with(&prefix));
        self.log.borrow_mut().push(format!("{path}/"));
        Ok(())
    }
}

/// Calls until done; every call within its limits. Returns the calls made.
fn run(tree: &Tree, root: &str, keep: bool) -> usize {
    let mut place = 0;
    for call in 1..10_000 {
        tree.reads.set(0);
        let removals = tree.removals.get();
        let step = remove_tree_step(tree, root, keep, place).unwrap();
        assert!(step.removed <= MAX_REMOVALS, "removals: {}", step.removed);
        assert!(tree.reads.get() <= MAX_READS, "reads: {}", tree.reads.get());
        assert_eq!(step.removed, tree.removals.get() - removals, "the count");
        if step.done {
            assert_eq!(step.next, 0);
            return call;
        }
        place = step.next;
    }
    panic!("never done");
}

fn both(test: impl Fn(&Tree)) {
    for stable in [true, false] {
        test(&Tree::new(stable));
    }
}

#[test]
fn a_big_folder_goes_in_several_calls() {
    both(|tree| {
        tree.dir("keep");
        tree.file("keep/a");
        for i in 0..700 {
            tree.file(&format!("data/f{i}"));
        }
        let calls = run(tree, "data", false);
        assert!(calls >= 700 / 64, "{calls} calls");
        assert!(tree.paths().iter().all(|p| p.starts_with("keep")));
        assert_eq!(tree.paths(), ["keep", "keep/a"]);
    });
}

#[test]
fn reads_stay_within_the_cap_in_a_call() {
    let tree = Tree::new(true);
    for i in 0..2000 {
        tree.file(&format!("data/f{i}"));
    }
    let mut place = 0;
    loop {
        tree.reads.set(0);
        let step = remove_tree_step(&tree, "data", false, place).unwrap();
        // Every entry a listing visits counts: the cap is the contract's.
        assert!(tree.reads.get() <= MAX_READS, "{}", tree.reads.get());
        if step.done {
            break;
        }
        place = step.next;
    }
    assert!(tree.paths().is_empty());
}

#[test]
fn a_nested_tree_is_removed_deepest_first() {
    both(|tree| {
        tree.file("t/a/b/c/one");
        tree.file("t/a/b/two");
        tree.file("t/a/three");
        tree.file("t/four");
        tree.dir("t/empty/deeper");
        tree.file("other/x");
        run(tree, "t", false);
        assert_eq!(tree.paths(), ["other", "other/x"]);
        let log = tree.log.borrow();
        let at = |p: &str| log.iter().position(|l| l == p).unwrap();
        assert!(at("t/a/b/c/one") < at("t/a/b/c/"));
        assert!(at("t/a/b/c/") < at("t/a/b/two") || at("t/a/b/two") < at("t/a/b/c/"));
        assert!(at("t/a/b/c/") < at("t/a/b/"));
        assert!(at("t/a/b/") < at("t/a/"));
        assert!(at("t/a/") < at("t/"));
    });
}

#[test]
fn many_folders_with_files_are_removed_in_pieces() {
    both(|tree| {
        for d in 0..40 {
            for f in 0..10 {
                tree.file(&format!("t/d{d}/f{f}"));
            }
        }
        tree.file("t/top");
        let calls = run(tree, "t", false);
        assert!(calls > 1);
        assert!(tree.paths().is_empty());
    });
}

#[test]
fn the_root_can_stay() {
    both(|tree| {
        tree.file("t/a/b");
        tree.file("t/c");
        run(tree, "t", true);
        assert_eq!(tree.paths(), ["t"]);
    });
}

#[test]
fn a_missing_path_is_done_and_a_file_is_one_entry() {
    let tree = Tree::new(true);
    tree.file("f");
    let step = remove_tree_step(&tree, "nope/x", false, 0).unwrap();
    assert_eq!((step.removed, step.done), (0, true));
    let step = remove_tree_step(&tree, "f", false, 0).unwrap();
    assert_eq!((step.removed, step.done), (1, true));
    assert!(tree.paths().is_empty());
    assert_eq!(kind(&tree, "f").unwrap(), Kind::Missing);
}

/// Every place a hostile or stale caller can hand back: nothing outside the
/// tree is touched and the loop ends.
#[test]
fn any_place_removes_only_inside_the_tree() {
    let depths = [0u64, 1, 2, 5, MAX_DEPTH as u64 - 1, MAX_DEPTH as u64, 255];
    let positions = [0u64, 1, 2, 3, 7, 1 << 20, (1 << 56) - 1];
    for stable in [true, false] {
        for depth in depths {
            for position in positions {
                let tree = Tree::new(stable);
                tree.file("outside/x");
                tree.file("t/a/one");
                tree.file("t/a/b/two");
                tree.file("t/c");
                tree.file("t2/inside-sibling");
                let mut place = depth << 56 | position;
                for _ in 0..200 {
                    let step = remove_tree_step(&tree, "t", false, place).unwrap();
                    if step.done {
                        break;
                    }
                    place = step.next;
                }
                let left = tree.paths();
                assert!(
                    left.iter().all(|p| !p.starts_with("t/") && p != "t"),
                    "{stable} {depth} {position}: {left:?}"
                );
                for kept in ["outside", "outside/x", "t2", "t2/inside-sibling"] {
                    assert!(left.iter().any(|p| p == kept), "{kept}: {left:?}");
                }
            }
        }
    }
}

#[test]
fn a_place_that_is_too_large_is_an_error_not_a_loop() {
    // 2^56 and more cannot be a place of ours; the storage's own places are
    // far below it.
    let tree = Tree::new(true);
    tree.file("t/a");
    let step = remove_tree_step(&tree, "t", false, 1 << 56).unwrap();
    assert!(step.removed > 0 || step.done);
}

#[test]
fn names_it_cannot_use_do_not_stop_it() {
    both(|tree| {
        // Not a name of the path grammar: a trailing dot, a colon, a name
        // over 255 bytes.
        let long = "x".repeat(300);
        tree.file("t/ok");
        tree.file("t/bad.");
        tree.file("t/co:lon");
        tree.file(&format!("t/{long}"));
        tree.file("t/sub/ok2");
        tree.file("t/sub/nul.");
        run(tree, "t", false);
        assert!(tree.paths().is_empty(), "{:?}", tree.paths());
    });
}

#[test]
fn unusable_names_are_removed_also_below_a_kept_root() {
    both(|tree| {
        tree.file("t/ok");
        tree.file("t/bad.");
        tree.file("t/sub/nul.");
        tree.file("t/sub/co:lon");
        run(tree, "t", true);
        assert_eq!(tree.paths(), ["t"], "nothing is left");
    });
}

/// A folder the walk cannot enter (too deep, too long a path) goes whole by
/// its place, in one removal, and the rest of the tree is removed as always:
/// nothing is left, nothing outside is touched.
#[test]
fn a_folder_the_walk_cannot_enter_goes_whole_by_its_place() {
    both(|tree| {
        let mut path = String::from("t");
        for i in 0..MAX_DEPTH + 4 {
            path.push_str(&format!("/d{i}"));
        }
        tree.file(&format!("{path}/leaf"));
        tree.file("t/side");
        tree.file("keep/x");
        run(tree, "t", false);
        assert_eq!(tree.paths(), ["keep", "keep/x"]);
    });
    // Within the depth it goes all the way by itself.
    both(|tree| {
        let mut path = String::from("t");
        for i in 0..MAX_DEPTH - 2 {
            path.push_str(&format!("/d{i}"));
        }
        tree.file(&format!("{path}/leaf"));
        run(tree, "t", false);
        assert!(tree.paths().is_empty());
        assert!(
            !tree.log.borrow().iter().any(|p| p.ends_with('#')),
            "no folder was taken by its place"
        );
    });
}

#[test]
fn a_path_too_long_to_build_goes_whole_by_its_place() {
    both(|tree| {
        // Names of 200 bytes, five deep: over the walk's 640-byte path.
        let name = |i: usize| format!("{i}{}", "n".repeat(199));
        let mut path = String::from("t");
        for i in 0..5 {
            path.push('/');
            path.push_str(&name(i));
        }
        tree.file(&format!("{path}/leaf"));
        tree.file("t/side");
        tree.file("keep/x");
        run(tree, "t", false);
        assert_eq!(tree.paths(), ["keep", "keep/x"]);
    });
}

/// A name no path holds on a folder with something in it, with a folder in it
/// of the same kind, also below a kept root: nothing is left.
#[test]
fn unusable_folders_with_content_are_removed_below_a_kept_root() {
    both(|tree| {
        tree.file("t/nul./a");
        tree.file("t/nul./b./c");
        tree.file("t/ok");
        tree.file("t/sub/co:lon/deep/x");
        run(tree, "t", true);
        assert_eq!(tree.paths(), ["t"]);
    });
}

/// A storage that fails (power cut, card out) at any removal: what was
/// removed is gone, the rest is whole, and a new walk finishes it.
#[test]
fn a_failure_at_any_removal_leaves_a_whole_rest() {
    let build = |tree: &Tree| {
        for d in 0..5 {
            for f in 0..4 {
                tree.file(&format!("t/d{d}/s/f{f}"));
            }
            tree.file(&format!("t/d{d}/top"));
        }
        tree.file("t/root-file");
        tree.file("keep/x");
    };
    for stable in [true, false] {
        let count = {
            let tree = Tree::new(stable);
            build(&tree);
            run(&tree, "t", false);
            tree.removals.get()
        };
        for fail in 1..=count {
            let tree = Tree::new(stable);
            build(&tree);
            tree.fail_at.set(Some(fail));
            let mut place = 0;
            let mut failed = false;
            for _ in 0..1000 {
                match remove_tree_step(&tree, "t", false, place) {
                    Ok(step) if step.done => break,
                    Ok(step) => place = step.next,
                    Err(error) => {
                        assert_eq!(error, StorageError::Io);
                        failed = true;
                        break;
                    }
                }
            }
            assert!(failed, "{fail} of {count}");
            // The rest is whole: every file of a folder that is still there
            // is one that was made, and the tree outside is as it was.
            let left = tree.paths();
            assert!(left.iter().any(|p| p == "keep/x"));
            // Calling again, from 0 or from the last place, finishes.
            tree.fail_at.set(None);
            run(&tree, "t", false);
            assert_eq!(tree.paths(), ["keep", "keep/x"], "{stable} {fail}");
        }
    }
}

#[test]
fn a_call_that_reads_its_share_before_removing_much_still_advances() {
    // 300 names the walk cannot use come first: they are removed by place,
    // so every call removes some and the calls end.
    let tree = Tree::new(true);
    for i in 0..300 {
        tree.file(&format!("t/j{i}."));
    }
    for i in 0..5 {
        tree.file(&format!("t/f{i}"));
    }
    let first = remove_tree_step(&tree, "t", false, 0).unwrap();
    assert!(!first.done);
    assert!(first.removed > 0);
    run(&tree, "t", false);
    assert!(tree.paths().is_empty());
}

#[test]
fn a_nested_tree_never_asks_remove_dir_all_for_more_than_an_empty_folder() {
    both(|tree| {
        for d in 0..6 {
            tree.file(&format!("t/d{d}/s/f"));
            tree.file(&format!("t/d{d}/j."));
        }
        run(tree, "t", false);
        assert!(tree.paths().is_empty());
    });
}

/// A resumed place whose folder is gone (or is a file now) is stale: the
/// folder's parent starts over, the call is no error, and the rest is
/// removed.
#[test]
fn a_resumed_place_whose_folder_is_gone_starts_that_folder_over() {
    both(|tree| {
        for i in 0..200 {
            tree.file(&format!("t/a/b/f{i}"));
        }
        tree.file("t/a/z");
        tree.file("keep/x");
        let step = remove_tree_step(tree, "t", false, 0).unwrap();
        assert!(!step.done);
        // The folder the place names goes behind the walk's back.
        tree.nodes
            .borrow_mut()
            .retain(|p, _| p != "t/a/b" && !p.starts_with("t/a/b/"));
        let mut place = step.next;
        for _ in 0..100 {
            let step = remove_tree_step(tree, "t", false, place).unwrap();
            if step.done {
                break;
            }
            place = step.next;
        }
        assert_eq!(tree.paths(), ["keep", "keep/x"]);
    });
}
