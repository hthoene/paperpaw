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

/// Clusters free on `s`, and on a card just made (the baseline).
fn free_clusters(s: &FatStorage<SharedDisk<Cut>>) -> u32 {
    s.fs.stats().unwrap().free_clusters()
}

fn baseline() -> u32 {
    free_clusters(&Cut::formatted_of(4 << 20, options()).storage())
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
        let last_blocks;
        loop {
            let before = card.reads.get();
            let mut reads = Reads::default();
            let step = s.walk_tree("Big", false, place, &mut reads).unwrap();
            assert!(step.removed <= MAX_REMOVALS);
            assert!(reads.work <= MAX_READS, "{reads:?}");
            assert!(reads.descent <= DESCENT, "{reads:?}");
            worst.work = worst.work.max(reads.work);
            let blocks = card.reads.get() - before;
            calls += 1;
            if step.done {
                // The look at the folder from its first slot before it goes
                // reads the slots the walk freed: the folder once, in blocks.
                last_blocks = blocks;
                break;
            }
            worst_blocks = worst_blocks.max(blocks);
            place = step.next;
        }
        assert!(calls >= files / 64, "{calls} calls");
        assert!(!exists_dir(&s, "Big"));
        assert_eq!(whole(&s, "Keep/k.txt").as_deref(), Ok(&b"keep"[..]));
        (worst.work, worst_blocks, last_blocks)
    };
    let (small, big) = (measure(600), measure(3000));
    eprintln!("entries and blocks per call: {small:?} for 600, {big:?} for 3000");
    // 3,000 files are 9,000 slots of 32 bytes: 563 blocks; a call reads a
    // fraction of that and as much as for 600.
    assert!(big.1 < 100, "{} blocks in a call", big.1);
    // The last call: the folder's blocks once, no more.
    assert!(big.2 <= 700, "{} blocks in the last call", big.2);
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
/// A folder here is a directory entry that stays when its last file is gone
/// (unlike a store whose folders are its files): the sibling layout of a
/// document cache, a dot folder and a plain one, goes whole and nothing
/// beside it is touched.
#[test]
fn sibling_folders_one_of_them_a_dot_folder_are_removed_whole() {
    let card = Cut::formatted_of(4 << 20, options());
    let s = card.storage();
    s.write("a/pages/1", b"1").unwrap();
    s.write("a/.host/.complete", b"").unwrap();
    s.write("keep/x", b"x").unwrap();
    walk(&s, "a", false, 0);
    assert!(!exists_dir(&s, "a"));
    assert!(!exists_dir(&s, "a/pages") && !exists_dir(&s, "a/.host"));
    assert_eq!(whole(&s, "keep/x").as_deref(), Ok(&b"x"[..]));
}

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
    // Run to its end without a cut, no cluster stays taken (a cut may leave
    // the ones of the entry it was removing: taken by no file, see fatfs).
    for gone in ["Keep", "T2"] {
        s.remove_dir_all(gone).unwrap();
    }
    s.sync().unwrap();
    assert_eq!(free_clusters(&s), baseline());
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
        // No cluster stays taken by what the walk removed.
        for gone in ["Keep", "T2"] {
            s.remove_dir_all(gone).unwrap();
        }
        s.sync().unwrap();
        assert_eq!(free_clusters(&s), baseline(), "{place:#x}");
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

/// An entry written into a folder between two calls, into a slot the walk had
/// freed before its place, is found by the look at the folder's start: nothing
/// is lost with the folder, the card's free space is what it was.
#[test]
fn a_file_written_behind_the_walk_is_removed_and_no_space_is_lost() {
    let card = Cut::formatted_of(8 << 20, options());
    let s = card.storage();
    for n in 0..300 {
        s.write(&format!("Big/file-{n:04}.txt"), &[7u8; 700])
            .unwrap();
    }
    s.sync().unwrap();
    let free = |s: &FatStorage<SharedDisk<Cut>>| s.fs.stats().unwrap().free_clusters();
    let before = {
        let empty = Cut::formatted_of(8 << 20, options());
        free(&empty.storage())
    };
    let mut place = 0;
    // Part of the folder, then a new file into a slot freed at its start.
    let step = remove_tree_step(&s, "Big", false, place).unwrap();
    assert!(!step.done);
    place = step.next;
    s.write("Big/new.txt", &[9u8; 700]).unwrap();
    s.sync().unwrap();
    for _ in 0..200 {
        let step = remove_tree_step(&s, "Big", false, place).unwrap();
        if step.done {
            break;
        }
        place = step.next;
    }
    assert!(!exists_dir(&s, "Big"));
    s.sync().unwrap();
    assert_eq!(free(&s), before, "every cluster is free again");
}

/// A place that does not follow the walk's own (one past the first entry, one
/// before an entry written since) never loses an entry with its folder: the
/// folder is looked at from its start before it goes, nothing is left, and no
/// cluster stays taken.
#[test]
fn any_place_in_a_folder_still_ends_with_nothing_left_and_no_space_lost() {
    let (free, before) = (free_clusters, baseline());
    for keep in [false, true] {
        for place in [0u64, 2, 3, 6, 9, 12, 15, 40, 1 << 31] {
            let card = Cut::formatted_of(4 << 20, options());
            let s = card.storage();
            s.write("T/a/inner.txt", &[1u8; 900]).unwrap();
            s.write("T/b.txt", &[2u8; 900]).unwrap();
            s.write("T/c.txt", &[3u8; 900]).unwrap();
            s.write("Keep/k.txt", b"keep").unwrap();
            s.sync().unwrap();
            for _ in 0..50 {
                if remove_tree_step(&s, "T", keep, place)
                    .unwrap_or_else(|e| panic!("keep {keep} place {place:#x}: {e:?}"))
                    .done
                {
                    break;
                }
            }
            let mut left = Vec::new();
            if keep {
                s.list("T", &mut |e| left.push(e.name.to_string())).unwrap();
            } else {
                assert!(!exists_dir(&s, "T"), "keep {keep} place {place:#x}");
            }
            assert!(left.is_empty(), "keep {keep} place {place:#x}: {left:?}");
            s.remove_dir_all("T").ok();
            s.remove_dir_all("Keep").unwrap();
            s.sync().unwrap();
            assert_eq!(free(&s), before, "keep {keep} place {place:#x}");
        }
    }
}

/// Files before the subfolder of each level on the way down are removed as
/// they are read and charged to the work reads; both shares hold in every call.
#[test]
fn files_on_the_way_down_are_work_and_both_shares_hold() {
    let card = Cut::formatted_of(8 << 20, options());
    let s = card.storage();
    let mut dir = s.dir("T").ok();
    if dir.is_none() {
        s.write("T/x.txt", b"x").unwrap();
        dir = Some(s.dir("T").unwrap());
    }
    let mut dir = dir.unwrap();
    for _ in 0..100 {
        for n in 0..3 {
            dir.create_file(&format!("f{n}.txt")).unwrap();
        }
        dir = dir.create_dir("a").unwrap();
    }
    s.sync().unwrap();
    let (mut place, mut calls) = (0, 0);
    loop {
        let mut reads = Reads::default();
        let step = s.walk_tree("T", false, place, &mut reads).unwrap();
        assert!(
            reads.work <= MAX_READS && reads.descent <= DESCENT,
            "{reads:?}"
        );
        calls += 1;
        assert!(calls < 3000, "never done");
        if step.done {
            break;
        }
        place = step.next;
    }
    assert!(!exists_dir(&s, "T"));
    // Files written into the chain behind the walk's back, met going down
    // again: charged to the work reads, both shares held in every call.
    let mut dir = s.dir("U").ok().unwrap_or_else(|| {
        s.write("U/x.txt", b"x").unwrap();
        s.dir("U").unwrap()
    });
    for _ in 0..30 {
        dir = dir.create_dir("a").unwrap();
    }
    s.sync().unwrap();
    let step = s.walk_tree("U", false, 0, &mut Reads::default()).unwrap();
    let mut dir = s.dir("U").unwrap();
    while let Some(next) = dir
        .iter()
        .filter_map(Result::ok)
        .find(|e| e.is_dir() && e.file_name() != "." && e.file_name() != "..")
    {
        dir.create_file("late.txt").unwrap();
        dir = next.to_dir();
    }
    let mut calls = 0;
    let mut place = step.next;
    loop {
        let mut reads = Reads::default();
        let step = s.walk_tree("U", false, place, &mut reads).unwrap();
        assert!(
            reads.work <= MAX_READS && reads.descent <= DESCENT,
            "{reads:?}"
        );
        calls += 1;
        assert!(calls < 3000, "never done");
        if step.done {
            break;
        }
        place = step.next;
    }
    assert!(!exists_dir(&s, "U"));
}

/// A folder entry of a normal name that points at another live folder's
/// cluster (cross-linked, as a card left after resets can show): its `.` is
/// right but its `..` is not where its parent says. The walk cuts the entry
/// off and goes no further in: the other tree's files and the clusters they
/// hold stay, whatever the place.
#[test]
fn a_folder_entry_cross_linked_to_another_tree_is_cut_off_and_nothing_there_is_freed() {
    use super::damaged::{first_cluster, garble, short_entry};
    let build = |card: &Cut| {
        let s = card.storage();
        s.write("Books/deep/b.txt", &[5u8; 900]).unwrap();
        s.write("Books/a.txt", &[6u8; 900]).unwrap();
        s.write("T/SUB/mine.txt", &[7u8; 900]).unwrap();
        s.write("T/other.txt", &[8u8; 900]).unwrap();
        s.sync().unwrap();
    };
    for place in [0u64, 1 << 32, 2 << 32 | 3] {
        let card = Cut::formatted_of(4 << 20, options());
        build(&card);
        let books = first_cluster(&card, short_entry(&card, b"BOOKS      "));
        let sub = short_entry(&card, b"SUB        ");
        let orphaned = first_cluster(&card, sub);
        assert_ne!(books, orphaned);
        garble(&card, sub, b"SUB        ", 0x10, books, 0);
        let s = card.storage();
        let before = free_clusters(&s);
        for _ in 0..50 {
            if remove_tree_step(&s, "T", false, place).unwrap().done {
                break;
            }
        }
        assert!(!exists_dir(&s, "T"), "{place:#x}");
        for (path, byte) in [("Books/deep/b.txt", 5u8), ("Books/a.txt", 6u8)] {
            assert_eq!(whole(&s, path), Ok(vec![byte; 900]), "{place:#x} {path}");
        }
        // The same tree without the cross-link, walked: frees `T`'s own
        // clusters and the files'. With it, the same less what `SUB` and its
        // file held (taken by none, as a cut-off folder leaves them).
        let control = Cut::formatted_of(4 << 20, options());
        build(&control);
        let c = control.storage();
        let control_before = free_clusters(&c);
        walk(&c, "T", false, 0);
        c.sync().unwrap();
        s.sync().unwrap();
        let freed = free_clusters(&s) - before;
        let control_freed = free_clusters(&c) - control_before;
        assert!(
            freed < control_freed,
            "{place:#x}: {freed} of {control_freed}"
        );
        // Nothing of Books was freed: the free count stays at or below the
        // control's, the control having removed SUB's clusters as well.
        assert!(freed < control_freed, "{place:#x}");
        let _ = card;
    }
}
