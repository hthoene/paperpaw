//! V1 of the interface-7 storage contract (`docs/abi/proposals/
//! interface-7-events-storage-services.md`, section 2, Kernel position and
//! amendment S2): what power loss leaves of `write`, of a rename over a file
//! written by `append`, and of a torn `append` tail, at every block the
//! storage writes, across cluster sizes and FAT types. Test-only: nothing
//! here changes the storage.

use super::*;
use std::collections::BTreeMap;
use std::eprintln;
use std::rc::Rc;

/// A card to cut: a name for the report, its size, its cluster size and type.
struct Card {
    label: &'static str,
    size: usize,
    cluster: usize,
    fat32: bool,
}

/// Cluster sizes from one block to 8 KB (FAT12/16 on 4 MB), and a FAT32 card.
const CARDS: [Card; 6] = [
    Card {
        label: "4MB c512",
        size: 4 << 20,
        cluster: 512,
        fat32: false,
    },
    Card {
        label: "4MB c1024",
        size: 4 << 20,
        cluster: 1024,
        fat32: false,
    },
    Card {
        label: "4MB c2048",
        size: 4 << 20,
        cluster: 2048,
        fat32: false,
    },
    Card {
        label: "4MB c4096",
        size: 4 << 20,
        cluster: 4096,
        fat32: false,
    },
    Card {
        label: "4MB c8192",
        size: 4 << 20,
        cluster: 8192,
        fat32: false,
    },
    Card {
        label: "40MB FAT32 c512",
        size: 40 << 20,
        cluster: 512,
        fat32: true,
    },
];

impl Card {
    fn cluster(&self) -> usize {
        self.cluster
    }

    fn options(&self) -> FormatVolumeOptions {
        let options = FormatVolumeOptions::new().bytes_per_cluster(self.cluster as u32);
        if self.fat32 {
            options.fat_type(fatfs::FatType::Fat32)
        } else {
            options
        }
    }

    /// The card prepared by `setup`, synced: its image.
    fn prepared(&self, setup: impl FnOnce(&FatStorage<SharedDisk<Cut>>)) -> Vec<u8> {
        let card = Cut::formatted_of(self.size, self.options());
        let s = card.storage();
        setup(&s);
        s.sync().unwrap();
        card.image()
    }
}

/// A card as the base image with some blocks replaced. Mounting a card
/// leaks it (`Box::leak`, as on the device), so a whole image per cut costs
/// gigabytes on the 40 MB card; this costs the changed blocks only.
#[derive(Clone)]
struct Overlay {
    base: Rc<Vec<u8>>,
    changed: BTreeMap<u32, [u8; BLOCK]>,
    at: u64,
}

impl Overlay {
    fn block(&self, index: u32) -> [u8; BLOCK] {
        self.changed.get(&index).copied().unwrap_or_else(|| {
            let from = index as usize * BLOCK;
            let mut block = [0u8; BLOCK];
            block.copy_from_slice(&self.base[from..from + BLOCK]);
            block
        })
    }
}

impl std::io::Read for Overlay {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let left = (self.base.len() as u64).saturating_sub(self.at) as usize;
        let n = buf.len().min(left).min(BLOCK - self.at as usize % BLOCK);
        if n > 0 {
            let block = self.block((self.at / BLOCK as u64) as u32);
            let from = self.at as usize % BLOCK;
            buf[..n].copy_from_slice(&block[from..from + n]);
            self.at += n as u64;
        }
        Ok(n)
    }
}

impl std::io::Write for Overlay {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let left = (self.base.len() as u64).saturating_sub(self.at) as usize;
        let n = buf.len().min(left).min(BLOCK - self.at as usize % BLOCK);
        if n > 0 {
            let index = (self.at / BLOCK as u64) as u32;
            let mut block = self.block(index);
            let from = self.at as usize % BLOCK;
            block[from..from + n].copy_from_slice(&buf[..n]);
            self.changed.insert(index, block);
            self.at += n as u64;
        }
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl std::io::Seek for Overlay {
    fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
        use std::io::SeekFrom::{Current, End, Start};
        let at = match to {
            Start(n) => Some(n),
            End(d) => self
                .base
                .len()
                .checked_add_signed(d as isize)
                .map(|n| n as u64),
            Current(d) => self.at.checked_add_signed(d),
        };
        self.at = at.ok_or(std::io::ErrorKind::InvalidInput)?;
        Ok(self.at)
    }
}

/// The storage on a card of [`Overlay`].
fn mounted(card: Overlay) -> FatStorage<StdIoWrapper<Overlay>> {
    let fs = FileSystem::new(StdIoWrapper::from(card), FsOptions::new()).unwrap();
    FatStorage::new(Box::leak(Box::new(fs)))
}

/// Every state a card can be left in by `op`: the image before it and after
/// each block it writes.
struct Cuts {
    base: Rc<Vec<u8>>,
    writes: Vec<Written>,
}

fn cuts_of(image: Vec<u8>, op: impl FnOnce(&FatStorage<SharedDisk<Cut>>)) -> Cuts {
    let card = Cut::new(image);
    let s = card.storage();
    let base = Rc::new(card.image());
    card.writes.borrow_mut().clear();
    op(&s);
    let writes = card.writes.borrow().clone();
    Cuts { base, writes }
}

impl Cuts {
    fn len(&self) -> usize {
        self.writes.len() + 1
    }

    fn overlay(&self, changed: BTreeMap<u32, [u8; BLOCK]>) -> Overlay {
        Overlay {
            base: Rc::clone(&self.base),
            changed,
            at: 0,
        }
    }

    /// The cards in order, each built when asked for.
    fn iter(&self) -> impl Iterator<Item = Overlay> + '_ {
        let mut changed = BTreeMap::new();
        let mut next = 0;
        core::iter::once(self.overlay(BTreeMap::new())).chain(core::iter::from_fn(move || {
            let (index, block) = self.writes.get(next)?;
            next += 1;
            changed.insert(*index, *block);
            Some(self.overlay(changed.clone()))
        }))
    }

    /// The card once everything is written.
    fn last(&self) -> Overlay {
        self.iter().last().unwrap()
    }
}

/// `len` bytes that are neither 0 nor [`JUNK`], so that a tail of freed or
/// never-written clusters cannot be taken for data: 1..=200, repeating.
fn data(len: usize, seed: usize) -> Vec<u8> {
    (0..len).map(|i| 1 + ((i + seed) % 200) as u8).collect()
}

/// What a deleted file leaves in the clusters it frees.
const JUNK: u8 = 0xDD;

const A: &str = "Books/A rather long name.txt";

// --- 1. write: old or new ------------------------------------------------

/// `write` replaces a file whole: at every cut it holds the old content or
/// the new one, never a mix, an empty file or none, and its neighbours keep
/// theirs.
#[test]
fn v1_write_yields_old_or_new_at_every_cut() {
    for card in &CARDS {
        let c = card.cluster();
        // Sizes that end inside a cluster and span several, new shorter and longer.
        for (old_len, new_len) in [
            (3 * c + c / 2, 2 * c + c / 4),
            (c / 2, 3 * c + 7),
            (c, c),
            (5 * c, 1),
        ] {
            let (old, new) = (data(old_len, 3), data(new_len, 91));
            let image = card.prepared(|s| {
                s.write(A, &old).unwrap();
                s.write("keep.bin", &data(1000, 7)).unwrap();
            });
            let cuts = cuts_of(image, |s| s.write(A, &new).unwrap());
            let mut seen = [0usize; 2];
            for (n, image) in cuts.iter().enumerate() {
                let s = mounted(image);
                let a = whole(&s, A);
                assert!(
                    a == Ok(old.clone()) || a == Ok(new.clone()),
                    "{} old {old_len} new {new_len} cut {n}: {:?}",
                    card.label,
                    a.as_ref().map(Vec::len)
                );
                seen[usize::from(a == Ok(new.clone()) && old != new)] += 1;
                assert_eq!(whole(&s, "keep.bin"), Ok(data(1000, 7)), "{n}");
                assert_eq!(cross_linked(&s), Vec::<String>::new(), "{n}");
            }
            let last = mounted(cuts.last());
            assert_eq!(whole(&last, A), Ok(new.clone()), "{}", card.label);
            eprintln!(
                "V1.1 write {:<16} old {old_len:>5} new {new_len:>5}: {:>3} cuts, old {:>3}, new {:>3}",
                card.label,
                cuts.len(),
                seen[0],
                seen[1]
            );
        }
    }
}

// --- 2. rename over a file written by append ----------------------------

/// The pieces of `data` appended in chunks of `chunk` bytes.
fn appended(s: &FatStorage<SharedDisk<Cut>>, path: &str, data: &[u8], chunk: usize) {
    for piece in data.chunks(chunk) {
        s.append(path, piece).unwrap();
    }
}

/// A file written by `append` (and synced) and moved over another: the
/// target holds the old content or the new one, never a lost or mixed file,
/// at every cut; the new content is never lost while the target is old.
#[test]
fn v1_rename_over_a_file_written_by_append_is_old_or_new_at_every_cut() {
    for card in &CARDS {
        let c = card.cluster();
        let (old, new) = (data(3 * c + c / 2, 5), data(2 * c + c / 3, 77));
        for chunk in [37, c, 3 * c / 2] {
            let image = card.prepared(|s| {
                s.write(A, &old).unwrap();
                appended(s, "Books/a.part", &new, chunk);
            });
            let cuts = cuts_of(image, |s| s.rename("Books/a.part", A).unwrap());
            let (mut old_n, mut new_n) = (0, 0);
            for (n, image) in cuts.iter().enumerate() {
                let s = mounted(image);
                let a = whole(&s, A);
                assert!(
                    a == Ok(old.clone()) || a == Ok(new.clone()),
                    "{} chunk {chunk} cut {n}: {:?}",
                    card.label,
                    a.as_ref().map(Vec::len)
                );
                if a == Ok(old.clone()) {
                    old_n += 1;
                    // The new content is still where it was, or beside the target.
                    let part = whole(&s, "Books/a.part");
                    let beside = names(&s, "Books")
                        .into_iter()
                        .map(|(name, ..)| whole(&s, &format!("Books/{name}")))
                        .any(|content| content == Ok(new.clone()));
                    assert!(
                        part == Ok(new.clone()) || beside,
                        "{} chunk {chunk} cut {n}: new content lost",
                        card.label
                    );
                } else {
                    new_n += 1;
                }
                assert_eq!(cross_linked(&s), Vec::<String>::new(), "{n}");
            }
            let last = mounted(cuts.last());
            assert_eq!(whole(&last, A), Ok(new.clone()));
            eprintln!(
                "V1.2a rename {:<16} chunk {chunk:>5}: {:>3} cuts, target old {old_n:>3}, new {new_n:>3}",
                card.label,
                cuts.len()
            );
        }
    }
}

/// The whole recipe of S1 in one operation, cut anywhere: remove the
/// temporary file, append the chunks, sync, rename. The published file is
/// the old content or exactly the new bytes, never a mix; a leftover
/// temporary file holding old data (cut by an earlier attempt) does not
/// reach the published file.
#[test]
fn v1_the_append_sync_rename_recipe_publishes_old_or_exactly_new() {
    for card in &CARDS {
        let c = card.cluster();
        let (old, new, stale) = (
            data(2 * c + 100, 11),
            data(3 * c + c / 2, 123),
            data(4 * c, 200),
        );
        for leftover in [false, true] {
            let image = card.prepared(|s| {
                s.write(A, &old).unwrap();
                if leftover {
                    s.write("Books/a.tmp", &stale).unwrap();
                }
            });
            let cuts = cuts_of(image, |s| {
                // Removing first is required (S1): a leftover would take the
                // new chunks after its stale bytes.
                match s.remove("Books/a.tmp") {
                    Ok(()) | Err(StorageError::NotFound) => {}
                    Err(e) => panic!("{e:?}"),
                }
                appended(s, "Books/a.tmp", &new, c / 2 + 13);
                s.sync().unwrap();
                s.rename("Books/a.tmp", A).unwrap();
            });
            let mut published = 0;
            for (n, image) in cuts.iter().enumerate() {
                let s = mounted(image);
                let a = whole(&s, A);
                assert!(
                    a == Ok(old.clone()) || a == Ok(new.clone()),
                    "{} leftover {leftover} cut {n}: {:?}",
                    card.label,
                    a.as_ref().map(Vec::len)
                );
                published += usize::from(a == Ok(new.clone()));
            }
            let last = mounted(cuts.last());
            assert_eq!(whole(&last, A), Ok(new.clone()), "{}", card.label);
            eprintln!(
                "V1.2b recipe {:<16} leftover {leftover:<5}: {:>3} cuts, new published at {published:>3}",
                card.label,
                cuts.len()
            );
        }
    }
}

// --- 3. the torn tail after append --------------------------------------

/// What a reader finds in the appended file after a cut.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Seen {
    /// The file is not there.
    Missing,
    /// Exactly the old content.
    Old,
    /// The old content and a proper prefix of the new bytes.
    Prefix,
    /// The old content and all new bytes.
    Full,
    /// The size covers more than was written and the rest reads as zeros.
    ZeroTail,
    /// The size covers more than was written and the rest is what a deleted
    /// file left in the clusters (stale bytes).
    Stale,
    /// The rest is neither: a mix, or the old content changed.
    Other,
    /// Reading the file fails.
    Unreadable,
}

/// What `path` shows on `image`, given the old content and the new bytes.
fn seen(image: Overlay, path: &str, old: &[u8], new: &[u8]) -> Seen {
    let s = mounted(image);
    let Ok(size) = s.size(path) else {
        return Seen::Missing;
    };
    let mut got = vec![0u8; size as usize];
    match s.read_at(path, 0, &mut got) {
        Ok(n) if n == got.len() => {}
        _ => return Seen::Unreadable,
    }
    let want: Vec<u8> = old.iter().chain(new).copied().collect();
    let common = got.iter().zip(&want).take_while(|(a, b)| a == b).count();
    if common == got.len() {
        return if got.len() == old.len() {
            Seen::Old
        } else if got.len() == want.len() {
            Seen::Full
        } else {
            Seen::Prefix
        };
    }
    if common < old.len() {
        return Seen::Other;
    }
    let tail = &got[common..];
    if tail.iter().all(|&b| b == 0) {
        Seen::ZeroTail
    } else if tail.contains(&JUNK) {
        Seen::Stale
    } else {
        Seen::Other
    }
}

/// A card on which a file of junk was written and then removed, so that
/// the clusters the appended file takes next hold stale bytes, and on which
/// `path` holds `old`.
fn with_stale_clusters(card: &Card, path: &str, old: &[u8], junk_len: usize) -> Vec<u8> {
    card.prepared(|s| {
        s.write("junk/log.old.csv", &vec![JUNK; junk_len]).unwrap();
        s.write(path, old).unwrap();
        s.sync().unwrap();
        s.remove("junk/log.old.csv").unwrap();
        s.sync().unwrap();
    })
}

/// How an append of `new` to `old` fares at every cut, as a count per
/// outcome. `op` is what runs between the card as prepared and the cut.
fn tally(
    card: &Card,
    old: &[u8],
    new: &[u8],
    op: impl FnOnce(&FatStorage<SharedDisk<Cut>>),
) -> (usize, BTreeMap<String, usize>) {
    const LOG: &str = "log.csv";
    let image = with_stale_clusters(card, LOG, old, 24 * card.cluster());
    let cuts = cuts_of(image, op);
    let mut counts = BTreeMap::new();
    for image in cuts.iter() {
        *counts
            .entry(format!("{:?}", seen(image, LOG, old, new)))
            .or_insert(0) += 1;
    }
    (cuts.len(), counts)
}

fn line(counts: &BTreeMap<String, usize>) -> String {
    counts
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Outcomes that must never occur after a cut. `Stale` and `ZeroTail` are
/// listed on purpose: measured, they never appear (S2: no host-framed
/// `append_record` is needed on this storage); if one ever does, S2 must be
/// decided again, not this list edited.
const BAD: [&str; 5] = ["Missing", "Other", "Unreadable", "Stale", "ZeroTail"];

/// Appended without a sync, cut at every block the storage writes: the file
/// reads as the old content, or the old content and a prefix of what was
/// appended. It is never lost and the old content is never changed. (Measured:
/// nothing of an unsynced append reaches the card, the file stays old.)
#[test]
fn v1_append_without_sync_leaves_a_prefix_never_a_lost_file() {
    for card in &CARDS {
        let c = card.cluster();
        let old = data(c / 2 + 3, 1);
        for new_len in [1, c / 2, c, 2 * c + 5, 6 * c + c / 3] {
            let new = data(new_len, 150);
            let (cuts, counts) = tally(card, &old, &new, |s| s.append("log.csv", &new).unwrap());
            for bad in BAD {
                assert!(
                    !counts.contains_key(bad),
                    "{} new {new_len}: {bad} after a cut: {}",
                    card.label,
                    line(&counts)
                );
            }
            eprintln!(
                "V1.3a append, no sync {:<16} new {new_len:>5}: {cuts:>3} cuts: {}",
                card.label,
                line(&counts)
            );
        }
    }
}

/// Appended and synced, cut at every block the sync writes: the same
/// observation as above for the case that matters, where the cut falls
/// inside the write-back.
#[test]
fn v1_a_cut_inside_sync_leaves_a_prefix_never_a_lost_file() {
    for card in &CARDS {
        let c = card.cluster();
        let old = data(c / 2 + 3, 1);
        for new_len in [1, c / 2, c, 2 * c + 5, 6 * c + c / 3] {
            let new = data(new_len, 150);
            let (cuts, counts) = tally(card, &old, &new, |s| {
                s.append("log.csv", &new).unwrap();
                s.sync().unwrap();
            });
            for bad in BAD {
                assert!(
                    !counts.contains_key(bad),
                    "{} new {new_len}: {bad} after a cut: {}",
                    card.label,
                    line(&counts)
                );
            }
            assert!(
                counts.contains_key("Full"),
                "{}: {}",
                card.label,
                line(&counts)
            );
            eprintln!(
                "V1.3b append + sync   {:<16} new {new_len:>5}: {cuts:>3} cuts: {}",
                card.label,
                line(&counts)
            );
        }
    }
}

/// An appended file pushed out of the open-file cache by more files than the
/// cache holds (`OPEN_FILES`), with no sync: whatever the eviction writes
/// back, a reader sees the old content, a prefix of the new bytes or all of
/// them, and the result is recorded (stale and zero-filled tails included).
#[test]
fn v1_an_appended_file_evicted_from_the_open_cache_after_any_cut() {
    for card in &CARDS {
        let c = card.cluster();
        let old = data(c / 2 + 3, 1);
        for new_len in [c / 2, 2 * c + 5] {
            let new = data(new_len, 150);
            let (cuts, counts) = tally(card, &old, &new, |s| {
                s.append("log.csv", &new).unwrap();
                for n in 0..=OPEN_FILES {
                    s.write(&format!("Books/e{n:02}.txt"), b"x").unwrap();
                }
            });
            for bad in BAD {
                assert!(
                    !counts.contains_key(bad),
                    "{} new {new_len}: {bad} after a cut: {}",
                    card.label,
                    line(&counts)
                );
            }
            eprintln!(
                "V1.3d append, evicted {:<16} new {new_len:>5}: {cuts:>3} cuts: {}",
                card.label,
                line(&counts)
            );
        }
    }
}

/// Several small appends (a log's rows) and one sync, cut anywhere: what a
/// reader sees is the old content and a whole number of appended bytes from
/// the start of the stream, never stale or zero-filled bytes.
#[test]
fn v1_rows_appended_one_by_one_then_synced_after_any_cut() {
    for card in &CARDS {
        let c = card.cluster();
        let old = data(c / 3, 2);
        let rows: Vec<Vec<u8>> = (0..40).map(|i| data(23 + i % 11, 40 + i)).collect();
        let new: Vec<u8> = rows.iter().flatten().copied().collect();
        let (cuts, counts) = tally(card, &old, &new, |s| {
            for row in &rows {
                s.append("log.csv", row).unwrap();
            }
            s.sync().unwrap();
        });
        for bad in BAD {
            assert!(
                !counts.contains_key(bad),
                "{}: {bad} after a cut: {}",
                card.label,
                line(&counts)
            );
        }
        eprintln!(
            "V1.3c 40 rows + sync  {:<16} new {:>5}: {cuts:>3} cuts: {}",
            card.label,
            new.len(),
            line(&counts)
        );
    }
}
