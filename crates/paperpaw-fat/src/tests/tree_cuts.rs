//! The bounded tree removal (`paperpaw_platform::tree`, the walk behind the
//! file service's `remove_tree`, `files.md` 2.5 and 7) on this storage: a
//! folder over the read cap goes in several calls, a cut at any block of any
//! call leaves a whole rest that the next walk finishes, a stale place or the
//! place of another tree removes nothing outside, and a folder moved and
//! cut off before its `..` was pointed up is removed without touching the
//! parent it still names.

use super::cuts_v1::{cuts_of, mounted};
use super::*;
use crate::walk::{DEPTH, DESCENT, Reads};
use paperpaw_platform::tree::{MAX_READS, MAX_REMOVALS, TreeStep};
use std::eprintln;

/// One call of the card's own walk.
fn remove_tree_step(
    s: &impl Storage,
    root: &str,
    keep_root: bool,
    place: u64,
) -> Result<TreeStep, StorageError> {
    s.remove_tree_step(root, keep_root, place)
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

/// The entries the walk reads from the folders, counted where rust-fatfs gives
/// them (not the ones a visitor is shown): in the folders being emptied at most
/// the contract's 256, finding them again at most the descent's share, from a
/// place and from the start; and the blocks
/// read from the card do not grow with the folder.
#[test]
fn a_call_reads_a_bounded_part_of_a_big_folder() {
    let measure = |files: usize| {
        let card = Cut::formatted_of(8 << 20, options());
        let s = card.storage();
        for n in 0..files {
            s.write(&format!("Big/file-{n:04}.txt"), b"x").unwrap();
        }
        s.write("Keep/k.txt", b"keep").unwrap();
        s.sync().unwrap();
        let (mut place, mut worst_blocks, mut calls) = (0, 0, 0);
        let mut worst = Reads::default();
        loop {
            let before = card.reads.get();
            let mut reads = Reads::default();
            let step = s.walk_tree("Big", false, place, &mut reads).unwrap();
            assert!(step.removed <= MAX_REMOVALS);
            assert!(reads.work <= MAX_READS, "{reads:?}");
            assert!(reads.descent <= DESCENT, "{reads:?}");
            worst.work = worst.work.max(reads.work);
            worst_blocks = worst_blocks.max(card.reads.get() - before);
            calls += 1;
            if step.done {
                break;
            }
            place = step.next;
        }
        assert!(calls >= files / 64, "{calls} calls");
        assert!(!exists_dir(&s, "Big"));
        assert_eq!(whole(&s, "Keep/k.txt").as_deref(), Ok(&b"keep"[..]));
        (worst.work, worst_blocks)
    };
    let (small, big) = (measure(600), measure(3000));
    eprintln!("entries and blocks per call: {small:?} for 600, {big:?} for 3000");
    // 3,000 files are 9,000 slots of 32 bytes: 563 blocks; a call reads a
    // fraction of that and as much as for 600.
    assert!(big.1 < 100, "{} blocks in a call", big.1);
    assert!(big.1 <= small.1 * 4, "{big:?} for 3000, {small:?} for 600");
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

/// Names a computer made that PaperPaw's paths refuse (`nul.txt`, a long
/// name, a trailing dot) are removed by their place, one at a time, and the
/// walk leaves nothing, also below a kept root and also cut at any block.
#[test]
fn names_no_path_holds_are_removed_by_their_place() {
    let card = Cut::formatted_of(4 << 20, options());
    let s = card.storage();
    s.write("T/ok.txt", b"ok").unwrap();
    s.write("T/sub/ok2.txt", b"ok").unwrap();
    s.write("Keep/k.txt", b"keep").unwrap();
    for (dir, name) in [
        ("T", "nul.txt"),
        ("T", "aux"),
        ("T/sub", "NUL"),
        ("T/sub", "lpt1.dat"),
    ] {
        s.sync().unwrap();
        s.dir(dir).unwrap().create_file(name).unwrap();
    }
    s.sync().unwrap();
    // One junk entry by its place: the listing's first.
    let mut shown = Vec::new();
    s.list("T", &mut |e| shown.push(e.name.to_string()))
        .unwrap();
    assert!(shown.iter().any(|n| n == "nul.txt"), "{shown:?}");
    let image = card.image();
    let cuts = cuts_of(image, |s| {
        walk(s, "T", true, 0);
        s.sync().unwrap();
    });
    for (n, cut) in cuts.iter().enumerate() {
        let check = mounted(cut.clone());
        assert_eq!(
            whole(&check, "Keep/k.txt").as_deref(),
            Ok(&b"keep"[..]),
            "cut {n}"
        );
        let again = mounted(cut.clone());
        walk(&again, "T", true, 0);
        let mut left = Vec::new();
        again
            .list("T", &mut |e| left.push(e.name.to_string()))
            .unwrap();
        assert!(left.is_empty(), "cut {n}: {left:?}");
        assert_eq!(
            whole(&again, "Keep/k.txt").as_deref(),
            Ok(&b"keep"[..]),
            "cut {n}"
        );
    }
}

#[test]
fn a_folder_that_is_not_empty_is_not_removed_by_its_place() {
    let card = Cut::formatted_of(4 << 20, options());
    let s = card.storage();
    s.write("T/sub/x.txt", b"x").unwrap();
    s.sync().unwrap();
    assert_eq!(s.remove_listed("T", 0), Err(StorageError::InvalidPath));
    assert_eq!(whole(&s, "T/sub/x.txt").as_deref(), Ok(&b"x"[..]));
    s.remove("T/sub/x.txt").unwrap();
    s.remove_listed("T", 0).unwrap();
    assert!(!exists_dir(&s, "T/sub"));
    assert_eq!(s.remove_listed("T", 0), Err(StorageError::NotFound));
    assert_eq!(s.remove_listed("T", 0x1234), Err(StorageError::NotFound));
}

/// Names no path holds on folders with content, nested, and a tree deeper
/// than the walk goes: nothing is left below a kept root, nothing outside is
/// touched, at any cut the rest is whole and the next walk finishes.
#[test]
fn junk_folders_with_content_and_deep_trees_are_gone_whole() {
    let card = Cut::formatted_of(4 << 20, options());
    let s = card.storage();
    s.write("T/ok.txt", b"ok").unwrap();
    s.write("Keep/k.txt", b"keep").unwrap();
    s.sync().unwrap();
    // `nul` with files and a folder below it; a deep valid tree.
    let junk = s.dir("T").unwrap().create_dir("nul").unwrap();
    junk.create_file("a.txt").unwrap();
    junk.create_dir("inner")
        .unwrap()
        .create_file("b.txt")
        .unwrap();
    let mut deep = String::from("T");
    for i in 0..60 {
        deep.push_str(&format!("/d{i}"));
    }
    s.write(&format!("{deep}/leaf.txt"), b"leaf").unwrap();
    s.sync().unwrap();
    let image = card.image();
    let cuts = cuts_of(image, |s| {
        walk(s, "T", true, 0);
        s.sync().unwrap();
    });
    for (n, cut) in cuts.iter().enumerate() {
        let check = mounted(cut.clone());
        assert_eq!(
            whole(&check, "Keep/k.txt").as_deref(),
            Ok(&b"keep"[..]),
            "cut {n}"
        );
        let again = mounted(cut.clone());
        walk(&again, "T", true, 0);
        let mut left = Vec::new();
        again
            .list("T", &mut |e| left.push(e.name.to_string()))
            .unwrap();
        assert!(left.is_empty(), "cut {n}: {left:?}");
        assert_eq!(
            whole(&again, "Keep/k.txt").as_deref(),
            Ok(&b"keep"[..]),
            "cut {n}"
        );
    }
}

/// A tree of `levels` folders one in the other below `at`, a file in the last
/// (made through rust-fatfs: its path would be too long for the storage's).
fn nest(s: &FatStorage<SharedDisk<Cut>>, at: &str, levels: usize) {
    let mut dir = s.dir(at).unwrap();
    for _ in 0..levels {
        dir = dir.create_dir("a").unwrap();
    }
    dir.create_file("leaf.txt").unwrap();
}

/// A tree as deep as the walk goes is removed, a call at a time each within its
/// reads (finding the folder again included) and removals, nothing outside
/// touched.
#[test]
fn a_tree_as_deep_as_the_walk_goes_is_removed_in_bounded_calls() {
    let card = Cut::formatted_of(4 << 20, options());
    let s = card.storage();
    s.write("T/side.txt", b"side").unwrap();
    s.write("Keep/k.txt", b"keep").unwrap();
    nest(&s, "T", DEPTH);
    s.sync().unwrap();
    let (mut place, mut calls) = (0, 0);
    loop {
        let mut reads = Reads::default();
        let step = s.walk_tree("T", false, place, &mut reads).unwrap();
        assert!(
            reads.work <= MAX_READS && reads.descent <= DESCENT,
            "{reads:?}"
        );
        assert!(step.removed <= MAX_REMOVALS);
        calls += 1;
        assert!(calls < 3000, "never done");
        if step.done {
            break;
        }
        place = step.next;
    }
    eprintln!("{calls} calls for {DEPTH} levels");
    assert!(!exists_dir(&s, "T"));
    assert_eq!(whole(&s, "Keep/k.txt").as_deref(), Ok(&b"keep"[..]));
}

/// A tree deeper than the walk goes ends the call with `InvalidData` (the
/// service's `ERR_STATE`); what was removed stays removed and nothing outside is
/// touched.
#[test]
fn a_tree_deeper_than_the_walk_goes_is_an_error_and_touches_nothing_outside() {
    let card = Cut::formatted_of(4 << 20, options());
    let s = card.storage();
    s.write("T/side.txt", b"side").unwrap();
    s.write("Keep/k.txt", b"keep").unwrap();
    nest(&s, "T", DEPTH + 1);
    s.sync().unwrap();
    let mut place = 0;
    let error = loop {
        match remove_tree_step(&s, "T", false, place) {
            Ok(step) if step.done => panic!("done"),
            Ok(step) => place = step.next,
            Err(error) => break error,
        }
    };
    assert_eq!(error, StorageError::InvalidData);
    assert_eq!(whole(&s, "Keep/k.txt").as_deref(), Ok(&b"keep"[..]));
}
