//! The bounded tree removal (`paperpaw_platform::tree`, the walk behind the
//! file service's `remove_tree`, `files.md` 2.5 and 7) on this storage: a
//! folder over the read cap goes in several calls, a cut at any block of any
//! call leaves a whole rest that the next walk finishes, a stale place or the
//! place of another tree removes nothing outside, and a folder moved and
//! cut off before its `..` was pointed up is removed without touching the
//! parent it still names.

use super::cuts_v1::{cuts_of, mounted};
use super::*;
use paperpaw_platform::tree::{MAX_READS, MAX_REMOVALS, remove_tree_step};
use std::cell::Cell;

/// The storage, counting the entries its listings show.
struct Counted<'a, S: Storage> {
    inner: &'a S,
    reads: Cell<u32>,
}

impl<S: Storage> Storage for Counted<'_, S> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.inner.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove_dir_all(path)
    }
    fn list_from(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        self.inner.list_from(dir, from, &mut |entry| {
            self.reads.set(self.reads.get() + 1);
            visit(entry)
        })
    }
}

fn options() -> FormatVolumeOptions {
    FormatVolumeOptions::new().bytes_per_cluster(512)
}

/// Walks `root` to its end from `place`; the calls made. Every call within its
/// limits.
fn walk(s: &impl Storage, root: &str, keep: bool, mut place: u64) -> usize {
    for call in 1..2000 {
        let step = remove_tree_step(s, root, keep, place).unwrap();
        assert!(step.removed <= MAX_REMOVALS);
        if step.done {
            return call;
        }
        place = step.next;
    }
    panic!("never done: {root}");
}

/// The tree below `T`: folders of files, a nested folder, and files at the top.
fn tree(s: &FatStorage<SharedDisk<Cut>>) {
    for d in 0..6 {
        for f in 0..14 {
            s.write(&format!("T/d{d}/f{f:02}.txt"), b"x").unwrap();
        }
    }
    s.write("T/d2/deep/er/leaf.txt", b"leaf").unwrap();
    s.write("T/top.txt", b"top").unwrap();
    // What the walk must never touch: beside it, a sibling of the same
    // name inside another folder, and a name that merely starts alike.
    s.write("Keep/k.txt", b"keep").unwrap();
    s.write("Keep/T/inner.txt", b"inner").unwrap();
    s.write("T2/other.txt", b"other").unwrap();
}

fn assert_outside(s: &dyn Storage, context: &str) {
    for (path, data) in [
        ("Keep/k.txt", &b"keep"[..]),
        ("Keep/T/inner.txt", b"inner"),
        ("T2/other.txt", b"other"),
    ] {
        assert_eq!(whole(s, path).as_deref(), Ok(data), "{context}: {path}");
    }
}

fn exists_dir(s: &dyn Storage, path: &str) -> bool {
    s.list(path, &mut |_| {}).is_ok()
}

#[test]
fn a_folder_over_the_read_cap_goes_in_several_calls() {
    let card = Cut::formatted_of(4 << 20, options());
    let s = card.storage();
    for n in 0..700 {
        s.write(&format!("Big/f{n:03}.txt"), b"x").unwrap();
    }
    s.write("Keep/k.txt", b"keep").unwrap();
    let counted = Counted {
        inner: &s,
        reads: Cell::new(0),
    };
    let (mut place, mut calls) = (0, 0);
    loop {
        counted.reads.set(0);
        let step = remove_tree_step(&counted, "Big", false, place).unwrap();
        calls += 1;
        assert!(step.removed <= MAX_REMOVALS);
        // The entries shown: those taken, one look-ahead each, and a check
        // that a folder is empty; never the folder whole.
        assert!(
            counted.reads.get() <= 2 * MAX_READS + 4,
            "{}",
            counted.reads.get()
        );
        if step.done {
            break;
        }
        place = step.next;
    }
    assert!(calls >= 700 / 64, "{calls} calls");
    assert!(!exists_dir(&s, "Big"));
    assert_eq!(whole(&s, "Keep/k.txt").as_deref(), Ok(&b"keep"[..]));
}

#[test]
fn the_kept_root_of_a_big_folder_is_emptied() {
    let card = Cut::formatted_of(4 << 20, options());
    let s = card.storage();
    for n in 0..150 {
        s.write(&format!("Data/sub/f{n:03}.txt"), b"x").unwrap();
        s.write(&format!("Data/g{n:03}.txt"), b"x").unwrap();
    }
    walk(&s, "Data", true, 0);
    let mut left = 0;
    s.list("Data", &mut |_| left += 1).unwrap();
    assert_eq!(left, 0, "the folder stays, empty");
}

/// A cut at any block of any call: what was removed is gone, the rest is
/// whole, nothing outside is touched, and the walk from the last place or from 0
/// finishes.
#[test]
fn a_cut_at_any_block_of_any_call_leaves_a_whole_rest() {
    let card = Cut::formatted_of(4 << 20, options());
    let s = card.storage();
    tree(&s);
    s.sync().unwrap();
    let mut place = 0;
    let (mut cut_states, mut calls) = (0, 0);
    loop {
        let image = card.image();
        let cuts = cuts_of(image, |s| {
            remove_tree_step(s, "T", false, place).unwrap();
            s.sync().unwrap();
        });
        for (n, cut) in cuts.iter().enumerate() {
            cut_states += 1;
            let context = format!("call {calls}, cut {n}");
            // Whole rest: every file still listed under `T` reads in full.
            let check = mounted(cut.clone());
            assert_outside(&check, &context);
            assert_whole(&check, "T", &context);
            drop(check);
            // From the place the plugin holds, and from the start.
            for from in [place, 0] {
                let after = mounted(cut.clone());
                walk(&after, "T", false, from);
                assert!(!exists_dir(&after, "T"), "{context} from {from}");
                assert_outside(&after, &context);
            }
        }
        let step = remove_tree_step(&s, "T", false, place).unwrap();
        s.sync().unwrap();
        calls += 1;
        if step.done {
            break;
        }
        place = step.next;
    }
    assert!(calls >= 2, "the tree takes several calls");
    assert!(cut_states > 50, "{cut_states} states");
}

/// Every file listed in the folders below `dir` reads in full: no entry
/// whose data is gone.
fn assert_whole(s: &dyn Storage, dir: &str, context: &str) {
    let mut found: Vec<(String, bool)> = Vec::new();
    if s.list(dir, &mut |e| found.push((e.name.to_string(), e.is_dir)))
        .is_err()
    {
        return;
    }
    for (name, is_dir) in found {
        let path = format!("{dir}/{name}");
        if is_dir {
            assert_whole(s, &path, context);
        } else {
            let data = whole(s, &path).unwrap_or_else(|e| panic!("{context}: {path}: {e:?}"));
            assert!(!data.is_empty(), "{context}: {path} lost its data");
        }
    }
}

#[test]
fn a_stale_place_or_the_place_of_another_tree_removes_nothing_outside() {
    // The places a walk of a bigger tree gave, used on a smaller one.
    let card = Cut::formatted_of(4 << 20, options());
    let s = card.storage();
    tree(&s);
    let mut places = vec![
        0u64,
        1,
        2,
        1 << 32,
        3 << 32 | 5,
        1 << 56,
        2 << 56 | 7,
        15 << 56,
        16 << 56,
        u64::MAX,
    ];
    let mut place = 0;
    loop {
        let step = remove_tree_step(&s, "T", false, place).unwrap();
        if step.done {
            break;
        }
        places.push(step.next);
        place = step.next;
    }
    for place in places {
        let card = Cut::formatted_of(4 << 20, options());
        let s = card.storage();
        s.write("T/a/b/one.txt", b"1").unwrap();
        s.write("T/a/two.txt", b"2").unwrap();
        s.write("T/three.txt", b"3").unwrap();
        s.write("Keep/k.txt", b"keep").unwrap();
        s.write("Keep/T/inner.txt", b"inner").unwrap();
        s.write("T2/other.txt", b"other").unwrap();
        walk(&s, "T", false, place);
        assert!(!exists_dir(&s, "T"), "{place:#x}");
        assert_outside(&s, &format!("{place:#x}"));
    }
}

/// A folder moved and cut off before its `..` was pointed up still names the
/// old parent: removing it goes down only, and the old parent stays.
#[test]
fn a_folder_with_dotdot_at_its_old_parent_is_removed_without_touching_it() {
    let card = Cut::formatted_of(4 << 20, options());
    let s = card.storage();
    for n in 0..5 {
        s.write(&format!("Old/Sub/in/f{n}.txt"), b"x").unwrap();
    }
    s.write("Old/Sub/top.txt", b"top").unwrap();
    s.write("Old/stay.txt", b"stay").unwrap();
    s.write("New/stay.txt", b"new stay").unwrap();
    s.sync().unwrap();
    let cuts = cuts_of(card.image(), |s| {
        s.rename("Old/Sub", "New/Sub").unwrap();
        s.sync().unwrap();
    });
    for (n, cut) in cuts.iter().enumerate() {
        for target in ["New/Sub", "Old/Sub"] {
            let s = mounted(cut.clone());
            if !exists_dir(&s, target) {
                continue;
            }
            walk(&s, target, false, 0);
            assert!(!exists_dir(&s, target), "cut {n}: {target}");
            assert_eq!(
                whole(&s, "Old/stay.txt").as_deref(),
                Ok(&b"stay"[..]),
                "cut {n} {target}"
            );
            assert_eq!(
                whole(&s, "New/stay.txt").as_deref(),
                Ok(&b"new stay"[..]),
                "cut {n} {target}"
            );
        }
    }
    assert!(cuts.len() > 2, "{} states", cuts.len());
}

#[test]
fn names_of_other_systems_are_removed_with_their_folder() {
    let card = Cut::formatted_of(4 << 20, options());
    let s = card.storage();
    s.write("T/ok.txt", b"ok").unwrap();
    s.write("T/sub/ok2.txt", b"ok").unwrap();
    s.sync().unwrap();
    walk(&s, "T", false, 0);
    assert!(!exists_dir(&s, "T"));
}
