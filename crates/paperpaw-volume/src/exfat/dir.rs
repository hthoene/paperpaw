//! Directories of an exFAT volume: entry sets read, added, changed and
//! removed, each change one block's write (see the [module
//! documentation](super)).

use alloc::string::String;
use alloc::vec::Vec;
use core::cell::Cell;

use super::layout::{
    self, ALLOCATION_POSSIBLE, ATTR_ARCHIVE, ATTR_DIRECTORY, BLOCK, ENTRY, ENTRY_FILE, ENTRY_NAME,
    ENTRY_STREAM, ENTRY_UNUSED, FAT_END, IN_USE, MAX_DIRECTORY, MAX_NAME, MAX_SET, NAME_UNITS,
    NO_FAT_CHAIN, NO_TIME, SECONDARY, u16_at, u32_at, u64_at,
};
use super::{Chain, ExFat, Res, XFile};
use crate::dir_entry::FileAttributes;
use crate::error::Error;
use crate::fs::ReadWriteSeek;
use crate::time::DateTime;

/// In the upper half of a file entry's time of last access while its set,
/// a file entry and a stream entry in two blocks, is being changed (see
/// [`ExFat::update`]): no date (month 15, day 0).
const PENDING: u16 = 0xABE0;
/// In a file entry's `Create10msIncrement` (0 to 199 otherwise): its time
/// of creation holds the caller's mark (see `File::set_mark`).
const MARKED: u8 = 0xEE;

/// Where an entry set lies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Loc {
    /// Its directory's first cluster.
    pub(crate) dir: u32,
    /// The byte of the directory where its file entry begins.
    pub(crate) at: u64,
    /// The cluster holding that byte.
    pub(crate) cluster: u32,
    /// Whether the directory's clusters lay one after the other when the
    /// set was found. Clusters a directory has stay where they are, so
    /// this holds for the set even after the directory grew into a chain.
    pub(crate) contiguous: bool,
    /// Its entries.
    pub(crate) count: u8,
}

/// The entries of one set.
pub(crate) struct Set {
    pub(crate) bytes: [u8; MAX_SET * ENTRY],
    pub(crate) count: usize,
}

impl Set {
    fn new(count: usize) -> Self {
        Self {
            bytes: [0; MAX_SET * ENTRY],
            count: count.min(MAX_SET),
        }
    }

    fn used(&self) -> &[u8] {
        &self.bytes[..self.count * ENTRY]
    }

    fn seal(&mut self) {
        let sum = layout::set_checksum(self.used());
        self.bytes[2..4].copy_from_slice(&sum.to_le_bytes());
    }

    /// Whether the entries are a file's set as the specification shapes it,
    /// with a stream that can lie on a volume of `clusters` clusters of
    /// `1 << shift` bytes.
    fn shaped(&self, clusters: u32, shift: u8) -> bool {
        let b = &self.bytes;
        if b[0] != ENTRY_FILE || usize::from(b[1]) + 1 != self.count || self.count < 3 {
            return false;
        }
        let name_len = usize::from(b[ENTRY + 3]);
        let names = name_len.div_ceil(NAME_UNITS);
        if b[ENTRY] != ENTRY_STREAM || name_len == 0 || 2 + names > self.count {
            return false;
        }
        let kind = |i: usize| b[i * ENTRY];
        if (2..2 + names).any(|i| kind(i) != ENTRY_NAME)
            || (2 + names..self.count).any(|i| kind(i) & (IN_USE | SECONDARY) != IN_USE | SECONDARY)
        {
            return false;
        }
        let (valid, first, len) = (
            u64_at(b, ENTRY + 8),
            u32_at(b, ENTRY + 20),
            u64_at(b, ENTRY + 24),
        );
        let on_volume = first >= 2 && first - 2 < clusters;
        valid <= len
            && len <= u64::from(clusters) << shift
            && if first == 0 { len == 0 } else { on_volume }
    }

    /// The time of last access as every set written here has it: the time
    /// of the last change.
    fn settle_access(&mut self) {
        let (head, _) = self.bytes.split_at_mut(ENTRY);
        let (modified, accessed) = head[12..20].split_at_mut(4);
        accessed.copy_from_slice(modified);
    }

    fn pending(&self) -> Option<u16> {
        (u16_at(&self.bytes, 18) == PENDING).then(|| u16_at(&self.bytes, 16))
    }
}

/// `name` as exFAT keeps it, and its hash.
fn encode<E>(name: &str) -> Result<(Vec<u16>, u16), Error<E>> {
    let len = name.encode_utf16().count();
    if len == 0 || len > MAX_NAME || name == "." || name == ".." {
        return Err(Error::InvalidFileNameLength);
    }
    let mut units = Vec::new();
    units
        .try_reserve_exact(len)
        .map_err(|_| Error::UnexpectedEof)?;
    let mut hash = 0;
    for unit in name.encode_utf16() {
        if !layout::valid_unit(unit) {
            return Err(Error::UnsupportedFileNameCharacter);
        }
        hash = layout::hash_unit(hash, layout::upcase(unit));
        units.push(unit);
    }
    Ok((units, hash))
}

/// What a new set says of its file.
struct Fresh {
    attrs: u16,
    created: (u32, u8),
    modified: u32,
    first: u32,
    len: u64,
    valid: u64,
    contiguous: bool,
}

/// The set of a file named `name`.
fn build(name: &[u16], hash: u16, f: &Fresh) -> Set {
    let names = name.len().div_ceil(NAME_UNITS);
    let mut set = Set::new(2 + names);
    let b = &mut set.bytes;
    b[0] = ENTRY_FILE;
    b[1] = (1 + names) as u8;
    b[4..6].copy_from_slice(&f.attrs.to_le_bytes());
    b[8..12].copy_from_slice(&f.created.0.to_le_bytes());
    b[12..16].copy_from_slice(&f.modified.to_le_bytes());
    b[16..20].copy_from_slice(&f.modified.to_le_bytes());
    b[20] = f.created.1;
    let s = &mut b[ENTRY..2 * ENTRY];
    s[0] = ENTRY_STREAM;
    s[1] = stream_flags(f.first, f.contiguous);
    s[3] = name.len() as u8;
    s[4..6].copy_from_slice(&hash.to_le_bytes());
    s[8..16].copy_from_slice(&f.valid.to_le_bytes());
    s[20..24].copy_from_slice(&f.first.to_le_bytes());
    s[24..32].copy_from_slice(&f.len.to_le_bytes());
    for (i, units) in name.chunks(NAME_UNITS).enumerate() {
        let e = &mut b[(2 + i) * ENTRY..(3 + i) * ENTRY];
        e[0] = ENTRY_NAME;
        for (k, unit) in units.iter().enumerate() {
            e[2 + 2 * k..4 + 2 * k].copy_from_slice(&unit.to_le_bytes());
        }
    }
    set.seal();
    set
}

fn stream_flags(first: u32, contiguous: bool) -> u8 {
    match (first, contiguous) {
        (0, _) | (_, false) => ALLOCATION_POSSIBLE,
        (_, true) => ALLOCATION_POSSIBLE | NO_FAT_CHAIN,
    }
}

/// Writes a stream's place and size into the stream entry of `set`.
pub(crate) fn set_stream(set: &mut Set, first: u32, len: u64, valid: u64, contiguous: bool) {
    let s = &mut set.bytes[ENTRY..2 * ENTRY];
    s[1] = stream_flags(first, contiguous);
    s[8..16].copy_from_slice(&valid.to_le_bytes());
    s[20..24].copy_from_slice(&first.to_le_bytes());
    s[24..32].copy_from_slice(&len.to_le_bytes());
}

/// Writes a caller's mark into the file entry of `set`.
pub(crate) fn set_mark(set: &mut Set, mark: u32) {
    set.bytes[8..12].copy_from_slice(&mark.to_le_bytes());
    set.bytes[20] = MARKED;
}

pub(crate) fn set_attrs(set: &mut Set, attrs: FileAttributes) {
    let keep = u16_at(&set.bytes, 4) & (ATTR_DIRECTORY | 0xFFC0);
    let attrs = keep | (u16::from(attrs.bits()) & 0x27);
    set.bytes[4..6].copy_from_slice(&attrs.to_le_bytes());
}

impl<IO: ReadWriteSeek> ExFat<IO> {
    /// Where entry `i` of the set at `loc` is on the volume.
    fn entry_pos(&self, loc: &Loc, i: usize) -> Res<u64, IO> {
        let size = self.cluster_size();
        let mut offset = (loc.at & (size - 1)) + (i * ENTRY) as u64;
        let mut cluster = loc.cluster;
        let chain = Chain {
            first: cluster,
            contiguous: loc.contiguous,
        };
        while offset >= size {
            cluster = self
                .next_in(chain, cluster)?
                .ok_or(Error::CorruptedFileSystem)?;
            offset -= size;
        }
        Ok(self.cluster_offset(cluster)? + offset)
    }

    /// The set at `loc` where it is a whole file's set, as it is to be
    /// read; and whether it was cut off in [`Self::update`] after its
    /// stream entry was written (its file entry is then still to write).
    /// `None` for entries that are no valid set: left alone.
    fn load(&self, loc: &Loc) -> Res<Option<(Set, bool)>, IO> {
        let mut set = Set::new(usize::from(loc.count));
        if usize::from(loc.count) > MAX_SET {
            return Ok(None);
        }
        for i in 0..set.count {
            let at = self.entry_pos(loc, i)?;
            self.read_at(at, &mut set.bytes[i * ENTRY..(i + 1) * ENTRY])?;
        }
        if !set.shaped(self.clusters(), self.cluster_shift()) {
            return Ok(None);
        }
        let stored = u16_at(&set.bytes, 2);
        let whole = layout::set_checksum(set.used()) == stored;
        let Some(promised) = set.pending() else {
            return Ok(whole.then_some((set, false)));
        };
        set.settle_access();
        if whole {
            // Cut off before the stream entry was written: the old set.
            set.seal();
            return Ok(Some((set, false)));
        }
        // The file entry said what the set's checksum would be once its
        // stream entry was written: it is, so the set is the new one.
        if layout::set_checksum(set.used()) == promised {
            set.seal();
            return Ok(Some((set, true)));
        }
        Ok(None)
    }

    /// Changes the file entry and the stream entry of the set at `loc`.
    ///
    /// In one block (every set written here), the two are one write. In
    /// two blocks (a set a computer placed so), three: the file entry
    /// with a promise of the checksum the set will have, the stream entry,
    /// the file entry. Cut off after the first, the set is the old one
    /// with a valid checksum; after the second, [`Self::load`] finds the
    /// promise kept and takes the new one. Nothing else makes a set with a
    /// wrong checksum valid.
    pub(crate) fn update(&self, loc: &Loc, change: impl FnOnce(&mut Set)) -> Res<(), IO> {
        let (mut set, _) = self.load(loc)?.ok_or(Error::CorruptedFileSystem)?;
        let mut old_stream = [0u8; ENTRY];
        old_stream.copy_from_slice(&set.bytes[ENTRY..2 * ENTRY]);
        change(&mut set);
        let (head, stream) = (self.entry_pos(loc, 0)?, self.entry_pos(loc, 1)?);
        if head / BLOCK == stream / BLOCK {
            set.seal();
            return self.write_at(head, &set.bytes[..2 * ENTRY]);
        }
        set.settle_access();
        set.seal();
        if set.bytes[ENTRY..2 * ENTRY] == old_stream {
            return self.write_at(head, &set.bytes[..ENTRY]);
        }
        let mut last = [0u8; 2 * ENTRY];
        last.copy_from_slice(&set.bytes[..2 * ENTRY]);
        // The old stream entry under a file entry that promises the new
        // set's checksum: a valid set.
        set.bytes[16..18].copy_from_slice(&last[2..4]);
        set.bytes[18..20].copy_from_slice(&PENDING.to_le_bytes());
        set.bytes[ENTRY..2 * ENTRY].copy_from_slice(&old_stream);
        set.seal();
        self.write_at(head, &set.bytes[..ENTRY])?;
        self.write_at(stream, &last[ENTRY..])?;
        self.write_at(head, &last[..ENTRY])
    }

    /// Takes the set at `loc` out of use, its file entry's block first:
    /// with that write the set is gone.
    fn clear(&self, loc: &Loc) -> Res<(), IO> {
        for i in 0..usize::from(loc.count) {
            let at = self.entry_pos(loc, i)?;
            let mut kind = [0u8; 1];
            self.read_at(at, &mut kind)?;
            let expected = match i {
                0 => kind[0] == ENTRY_FILE,
                _ => kind[0] & (IN_USE | SECONDARY) == IN_USE | SECONDARY,
            };
            if !expected {
                // Not the set that was found here: nothing of it is touched.
                return match i {
                    0 => Err(Error::NotFound),
                    _ => Ok(()),
                };
            }
            self.write_at(at, &[kind[0] & !IN_USE])?;
        }
        Ok(())
    }
}

/// An entry of a directory: a file or a directory.
pub struct XEntry<'a, IO: ReadWriteSeek> {
    fs: &'a ExFat<IO>,
    pub(crate) loc: Loc,
    attrs: u16,
    first: u32,
    len: u64,
    valid: u64,
    contiguous: bool,
    created: (u32, u8),
    modified: (u32, u8),
    hash: u16,
    name: Vec<u16>,
    /// Its name where a FAT volume would keep it as a short name too (12
    /// ASCII letters at most), and how many of the bytes are used.
    short: ([u8; 12], u8),
}

impl<'a, IO: ReadWriteSeek> XEntry<'a, IO> {
    fn of(fs: &'a ExFat<IO>, loc: Loc, set: &Set) -> Res<Self, IO> {
        let b = &set.bytes;
        let len = usize::from(b[ENTRY + 3]);
        let mut name = Vec::new();
        name.try_reserve_exact(len)
            .map_err(|_| Error::UnexpectedEof)?;
        name.extend((0..len).map(|i| {
            let entry = (2 + i / NAME_UNITS) * ENTRY;
            u16_at(b, entry + 2 + 2 * (i % NAME_UNITS))
        }));
        let mut short = ([0u8; 12], 0);
        if name.len() <= 12 && name.iter().all(|&unit| unit < 0x80) {
            for (to, unit) in short.0.iter_mut().zip(&name) {
                *to = *unit as u8;
            }
            short.1 = name.len() as u8;
        }
        Ok(Self {
            fs,
            loc,
            short,
            attrs: u16_at(b, 4),
            first: u32_at(b, ENTRY + 20),
            len: u64_at(b, ENTRY + 24),
            valid: u64_at(b, ENTRY + 8),
            contiguous: b[ENTRY + 1] & NO_FAT_CHAIN != 0,
            created: (u32_at(b, 8), b[20]),
            modified: (u32_at(b, 12), b[21]),
            hash: u16_at(b, ENTRY + 4),
            name,
        })
    }

    pub(crate) fn name_units(&self) -> &[u16] {
        &self.name
    }

    /// The name as a FAT volume's short name would hold it: for a caller
    /// that tells its own files by their short names (`~ABCDEFG.TMP`).
    /// Empty for a name no short name could be.
    pub(crate) fn short_name(&self) -> &[u8] {
        &self.short.0[..usize::from(self.short.1)]
    }

    pub(crate) fn file_name(&self) -> String {
        char::decode_utf16(self.name.iter().copied())
            .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect()
    }

    pub(crate) fn attributes(&self) -> FileAttributes {
        FileAttributes::from_bits_truncate(self.attrs as u8)
    }

    pub(crate) fn is_dir(&self) -> bool {
        self.attrs & ATTR_DIRECTORY != 0
    }

    pub(crate) fn first_cluster(&self) -> Option<u32> {
        (self.first != 0).then_some(self.first)
    }

    pub(crate) fn len(&self) -> u64 {
        self.len
    }

    pub(crate) fn modified(&self) -> DateTime {
        let (stamp, tens) = self.modified;
        DateTime::decode((stamp >> 16) as u16, stamp as u16, tens.min(199))
    }

    pub(crate) fn mark(&self) -> Option<u32> {
        (self.created.1 == MARKED).then_some(self.created.0)
    }

    pub(crate) fn offset_range(&self) -> (u64, u64) {
        (
            self.loc.at,
            self.loc.at + u64::from(self.loc.count) * ENTRY as u64,
        )
    }

    fn chain(&self) -> Chain {
        Chain {
            first: self.first,
            contiguous: self.contiguous,
        }
    }

    /// The clusters its size says it holds.
    fn clusters(&self) -> u32 {
        u32::try_from(self.len.div_ceil(self.fs.cluster_size())).unwrap_or(u32::MAX)
    }

    pub(crate) fn to_file(&self) -> XFile<'a, IO> {
        XFile::new(self.fs, self.chain(), self.len, self.valid, Some(self.loc))
    }

    pub(crate) fn to_dir(&self) -> XDir<'a, IO> {
        XDir {
            fs: self.fs,
            first: self.first,
            own: Some(self.loc),
            extent: Cell::new(None),
        }
    }

    fn is(&self, name: &[u16], hash: u16) -> bool {
        self.hash == hash
            && self.name.len() == name.len()
            && self
                .name
                .iter()
                .zip(name)
                .all(|(a, b)| layout::upcase(*a) == layout::upcase(*b))
    }

    /// What a set that takes this entry's file under another name says.
    fn fresh(&self) -> Fresh {
        Fresh {
            attrs: self.attrs,
            created: self.created,
            modified: self.modified.0,
            first: self.first,
            len: self.len,
            valid: self.valid,
            contiguous: self.contiguous,
        }
    }
}

/// A directory.
pub struct XDir<'a, IO: ReadWriteSeek> {
    fs: &'a ExFat<IO>,
    first: u32,
    /// Its own entry set, in the directory it is in; `None` for the root.
    own: Option<Loc>,
    /// Its clusters and its size as last read from its entry set, with the
    /// count of directories grown then (`ExFat::grown`): another handle
    /// may grow the directory.
    extent: Cell<Option<(Chain, u64, u64)>>,
}

impl<IO: ReadWriteSeek> Clone for XDir<'_, IO> {
    fn clone(&self) -> Self {
        Self {
            fs: self.fs,
            first: self.first,
            own: self.own,
            extent: self.extent.clone(),
        }
    }
}

impl<'a, IO: ReadWriteSeek> XDir<'a, IO> {
    pub(crate) fn root(fs: &'a ExFat<IO>) -> Self {
        Self {
            fs,
            first: fs.root(),
            own: None,
            extent: Cell::new(None),
        }
    }

    pub(crate) fn first_cluster(&self) -> u32 {
        self.first
    }

    /// Its clusters and its size in bytes (0 for the root: as far as the
    /// FAT links its clusters).
    fn extent(&self) -> Res<(Chain, u64), IO> {
        let grown = self.fs.grown();
        if let Some((chain, len, at)) = self.extent.get()
            && at == grown
        {
            return Ok((chain, len));
        }
        let (chain, len) = match &self.own {
            None => (
                Chain {
                    first: self.first,
                    contiguous: false,
                },
                0,
            ),
            Some(own) => {
                let (set, _) = self.fs.load(own)?.ok_or(Error::NotFound)?;
                let b = &set.bytes;
                if u16_at(b, 4) & ATTR_DIRECTORY == 0 || u32_at(b, ENTRY + 20) != self.first {
                    return Err(Error::NotFound);
                }
                // A directory has whole clusters, at least one: any other size is damage,
                // and growing would take its last cluster for one it does not end with.
                let len = u64_at(b, ENTRY + 24);
                if len == 0 || len % self.fs.cluster_size() != 0 {
                    return Err(Error::CorruptedFileSystem);
                }
                (
                    Chain {
                        first: self.first,
                        contiguous: b[ENTRY + 1] & NO_FAT_CHAIN != 0,
                    },
                    len.min(MAX_DIRECTORY),
                )
            }
        };
        self.extent.set(Some((chain, len, grown)));
        Ok((chain, len))
    }

    pub(crate) fn iter(&self) -> XIter<'a, IO> {
        self.iter_at(0)
    }

    pub(crate) fn iter_at(&self, at: u64) -> XIter<'a, IO> {
        let (chain, len, failed) = match self.extent() {
            Ok((chain, len)) => (chain, len, false),
            Err(_) => (
                Chain {
                    first: self.first,
                    contiguous: false,
                },
                0,
                true,
            ),
        };
        XIter {
            fs: self.fs,
            chain,
            len,
            at: at - at % ENTRY as u64,
            walked: (0, chain.first),
            failed,
            done: false,
        }
    }

    /// Whether an entry set (or a slot not in use, or the end) begins at
    /// byte `offset`.
    pub(crate) fn begins_entry(&self, offset: u64) -> Res<bool, IO> {
        if offset % ENTRY as u64 != 0 {
            return Ok(false);
        }
        let mut iter = self.iter_at(offset);
        Ok(match iter.slot()? {
            Some((_, entry)) => entry[0] & (IN_USE | SECONDARY) != IN_USE | SECONDARY,
            None => true,
        })
    }

    pub(crate) fn note(&self) -> Option<u64> {
        self.fs.note(self.first)
    }

    pub(crate) fn set_note(&self, note: u64) {
        self.fs.set_note(self.first, note);
    }

    pub(crate) fn entries_written(&self) -> u64 {
        self.fs.entries_written()
    }

    fn find_units(&self, name: &[u16], hash: u16) -> Res<Option<XEntry<'a, IO>>, IO> {
        for entry in self.iter() {
            let entry = entry?;
            if entry.is(name, hash) {
                return Ok(Some(entry));
            }
        }
        Ok(None)
    }

    pub(crate) fn find(&self, name: &str) -> Res<Option<XEntry<'a, IO>>, IO> {
        let (units, hash) = encode(name)?;
        self.find_units(&units, hash)
    }

    fn found(&self, name: &str) -> Res<XEntry<'a, IO>, IO> {
        self.find(name)?.ok_or(Error::NotFound)
    }

    pub(crate) fn is_empty(&self) -> Res<bool, IO> {
        match self.iter().next() {
            None => Ok(true),
            Some(Ok(_)) => Ok(false),
            Some(Err(e)) => Err(e),
        }
    }

    /// Adds `set` to the directory: into slots not in use, or at its end,
    /// which grows by a cluster where it must. The set's file entry and
    /// stream entry are put in one block (a slot is passed over for it),
    /// and the block of its file entry is written last, in one write:
    /// until then the directory reads as it did.
    fn add(&self, set: &Set) -> Res<Loc, IO> {
        let fs = self.fs;
        let (mut chain, len) = self.extent()?;
        let entry = ENTRY as u64;
        let need = set.count as u64 * entry;
        // Where a set beginning in a free run from `start` on is put.
        let placed = |start: u64| match start % BLOCK == BLOCK - entry {
            true => start + entry,
            false => start,
        };
        let mut iter = self.iter_at(0);
        let mut run: Option<u64> = None;
        let mut covered = 0u32;
        let mut reused = None;
        // The directory's end: where its end mark is, or its clusters end.
        let mut end = None;
        while let Some((at, e)) = iter.slot()? {
            let kind = e[0];
            let free = if kind == 0 {
                end = Some(at);
                break;
            } else if kind & IN_USE == 0 {
                covered = 0;
                true
            } else if kind & SECONDARY == 0 {
                covered = match kind {
                    layout::ENTRY_BITMAP | layout::ENTRY_UPCASE | layout::ENTRY_LABEL => 0,
                    _ => u32::from(e[1]),
                };
                false
            } else if covered > 0 {
                covered -= 1;
                false
            } else {
                // In use, counted by no entry before it: what a cut left
                // of a set added or removed.
                true
            };
            if free {
                let start = *run.get_or_insert(at);
                if at + entry >= placed(start) + need {
                    reused = Some(placed(start));
                    break;
                }
            } else {
                run = None;
            }
            iter.at += entry;
        }
        let capacity = iter.at.max(end.unwrap_or(0));
        let (at, tail) = match reused {
            Some(at) => (at, None),
            None => {
                // From the free slots before the end on, into what was
                // never written.
                let end = end.unwrap_or(capacity);
                let start = run.unwrap_or(end);
                let at = placed(start);
                let mut room = match len {
                    0 => self.chain_len(chain)?,
                    len => len,
                };
                while at + need > room {
                    room = self.grow(&mut chain, room)?;
                }
                (at, Some((end, room)))
            }
        };
        let mut walked = (0, chain.first);
        let cluster = fs
            .cluster_at(chain, &mut walked, (at >> fs.cluster_shift()) as u32)?
            .ok_or(Error::CorruptedFileSystem)?;
        let loc = Loc {
            dir: self.first,
            at,
            cluster,
            contiguous: chain.contiguous,
            count: set.count as u8,
        };
        let head = fs.entry_pos(&loc, 0)?;
        match tail {
            None => self.write_set(&loc, set, head)?,
            Some((end, room)) => {
                // What follows the set stays the directory's end, whatever
                // a cut left there.
                let after = at + need;
                if after < room {
                    let pos = self.slot_pos(chain, after)?;
                    let mut kind = [0u8; 1];
                    fs.read_at(pos, &mut kind)?;
                    if kind[0] != 0 {
                        fs.write_at(pos, &[0u8; ENTRY])?;
                    }
                }
                self.write_set(&loc, set, head)?;
                // A slot passed over at the end is the end no longer: with
                // this write the set is in the directory.
                if at > end {
                    fs.write_at(self.slot_pos(chain, end)?, &[ENTRY_UNUSED])?;
                }
            }
        }
        fs.entry_written(self.first);
        Ok(loc)
    }

    /// Where the slot at byte `at` of the directory is on the volume.
    fn slot_pos(&self, chain: Chain, at: u64) -> Res<u64, IO> {
        let fs = self.fs;
        let mut walked = (0, chain.first);
        let cluster = fs
            .cluster_at(chain, &mut walked, (at >> fs.cluster_shift()) as u32)?
            .ok_or(Error::CorruptedFileSystem)?;
        Ok(fs.cluster_offset(cluster)? + (at & (fs.cluster_size() - 1)))
    }

    /// Writes `set` at `loc`: the entries past the block of its file entry
    /// first, then that block's in one write.
    fn write_set(&self, loc: &Loc, set: &Set, head: u64) -> Res<(), IO> {
        let in_head = (((BLOCK - head % BLOCK) / ENTRY as u64) as usize).min(set.count);
        for i in in_head..set.count {
            let at = self.fs.entry_pos(loc, i)?;
            self.fs
                .write_at(at, &set.bytes[i * ENTRY..(i + 1) * ENTRY])?;
        }
        self.fs.write_at(head, &set.bytes[..in_head * ENTRY])
    }

    /// The bytes of a directory whose size is told by the FAT (the root).
    fn chain_len(&self, chain: Chain) -> Res<u64, IO> {
        let mut clusters = 1u64;
        let mut cluster = chain.first;
        while let Some(next) = self.fs.fat_next(cluster)? {
            cluster = next;
            clusters += 1;
            if clusters << self.fs.cluster_shift() > MAX_DIRECTORY {
                return Err(Error::CorruptedFileSystem);
            }
        }
        Ok(clusters << self.fs.cluster_shift())
    }

    /// Grows the directory of `room` bytes by a cluster, zeroed before
    /// anything links it; its new size.
    fn grow(&self, chain: &mut Chain, room: u64) -> Res<u64, IO> {
        let fs = self.fs;
        let size = fs.cluster_size();
        if room + size > MAX_DIRECTORY || room == 0 {
            return Err(Error::NotEnoughSpace);
        }
        let last_index = (room >> fs.cluster_shift())
            .checked_sub(1)
            .ok_or(Error::CorruptedFileSystem)?;
        let mut walked = (0, chain.first);
        let last = fs
            .cluster_at(*chain, &mut walked, last_index as u32)?
            .ok_or(Error::CorruptedFileSystem)?;
        let new = fs.take(Some(last.wrapping_add(1)))?;
        fs.zero(fs.cluster_offset(new)?, size)?;
        if !(chain.contiguous && new == last.wrapping_add(1)) {
            if chain.contiguous {
                for cluster in chain.first..last {
                    fs.fat_set(cluster, cluster + 1)?;
                }
                chain.contiguous = false;
            }
            fs.fat_set(new, FAT_END)?;
            fs.fat_set(last, new)?;
        }
        let room = room + size;
        if let Some(own) = &self.own {
            let chain = *chain;
            fs.update(own, |set| {
                set_stream(set, chain.first, room, room, chain.contiguous)
            })?;
        }
        fs.grew();
        self.extent.set(Some((
            *chain,
            if self.own.is_some() { room } else { 0 },
            fs.grown(),
        )));
        Ok(room)
    }

    /// File `name`, made empty where it is not there.
    pub(crate) fn create_file(&self, name: &str) -> Res<XFile<'a, IO>, IO> {
        let (units, hash) = encode(name)?;
        match self.find_units(&units, hash)? {
            Some(entry) if entry.is_dir() => Err(Error::InvalidInput),
            Some(entry) => Ok(entry.to_file()),
            None => {
                let set = build(&units, hash, &Fresh::empty(ATTR_ARCHIVE));
                let loc = self.add(&set)?;
                Ok(XFile::new(
                    self.fs,
                    Chain {
                        first: 0,
                        contiguous: true,
                    },
                    0,
                    0,
                    Some(loc),
                ))
            }
        }
    }

    /// Directory `name`, made where it is not there: its one cluster is
    /// taken and zeroed before its entry set names it.
    pub(crate) fn create_dir(&self, name: &str) -> Res<Self, IO> {
        let (units, hash) = encode(name)?;
        match self.find_units(&units, hash)? {
            Some(entry) if entry.is_dir() => Ok(entry.to_dir()),
            Some(_) => Err(Error::InvalidInput),
            None => {
                let fs = self.fs;
                let size = fs.cluster_size();
                let first = fs.take(None)?;
                fs.zero(fs.cluster_offset(first)?, size)?;
                let set = build(
                    &units,
                    hash,
                    &Fresh {
                        first,
                        len: size,
                        valid: size,
                        ..Fresh::empty(ATTR_DIRECTORY)
                    },
                );
                let own = match self.add(&set) {
                    Ok(own) => own,
                    Err(e) => {
                        // Not named by anything: given back.
                        fs.mark(first, 1, false)?;
                        return Err(e);
                    }
                };
                fs.drop_note(first);
                Ok(Self {
                    fs,
                    first,
                    own: Some(own),
                    extent: Cell::new(None),
                })
            }
        }
    }

    /// Whether `entry` is still on the card as it was found.
    fn check(&self, entry: &XEntry<'a, IO>) -> Res<(), IO> {
        let (set, _) = self.fs.load(&entry.loc)?.ok_or(Error::NotFound)?;
        let b = &set.bytes;
        if u16_at(b, ENTRY + 4) != entry.hash || u32_at(b, ENTRY + 20) != entry.first {
            return Err(Error::NotFound);
        }
        Ok(())
    }

    /// Removes `entry` (found in this directory, nothing changed in it
    /// since): its set first, then its clusters are given back.
    pub(crate) fn remove_found(&self, entry: &XEntry<'a, IO>, emptied: bool) -> Res<(), IO> {
        if entry.is_dir() && !emptied && !entry.to_dir().is_empty()? {
            return Err(Error::DirectoryIsNotEmpty);
        }
        self.check(entry)?;
        self.fs.clear(&entry.loc)?;
        if entry.is_dir() {
            self.fs.drop_note(entry.first);
        }
        self.fs.free(entry.chain(), entry.clusters())
    }

    pub(crate) fn remove(&self, name: &str) -> Res<(), IO> {
        self.remove_found(&self.found(name)?, false)
    }

    /// Removes the set of `entry`, leaving its clusters: another set
    /// holds them too.
    pub(crate) fn unlink_found(&self, entry: &XEntry<'a, IO>) -> Res<(), IO> {
        self.check(entry)?;
        if entry.is_dir() {
            self.fs.drop_note(entry.first);
        }
        self.fs.clear(&entry.loc)
    }

    pub(crate) fn unlink(&self, name: &str) -> Res<(), IO> {
        self.unlink_found(&self.found(name)?)
    }

    /// Moves `entry` of this directory to `name` in `to`: the new set is
    /// added before the old one is removed. Cut off between, the entry is
    /// at both places with the same clusters, never at neither.
    pub(crate) fn rename_found(
        &self,
        entry: &XEntry<'a, IO>,
        to: &Self,
        name: &str,
    ) -> Res<(), IO> {
        let (units, hash) = encode(name)?;
        if let Some(there) = to.find_units(&units, hash)? {
            return match there.loc == entry.loc {
                true => Ok(()),
                false => Err(Error::AlreadyExists),
            };
        }
        self.check(entry)?;
        to.add(&build(&units, hash, &entry.fresh()))?;
        if entry.is_dir() {
            self.fs.drop_note(entry.first);
        }
        self.fs.clear(&entry.loc)
    }

    pub(crate) fn rename(&self, from: &str, to: &Self, name: &str) -> Res<(), IO> {
        self.rename_found(&self.found(from)?, to, name)
    }

    /// Puts file `src` in the place of file `dst`, both in this
    /// directory, as `fatfs::Dir::replace` does: the new set of `dst` is
    /// added before the old one and that of `src` are removed, and the old
    /// clusters are given back last. Cut off, `dst` is the old file or the
    /// new one; called again, it finishes.
    pub(crate) fn replace(&self, src: &str, dst: &str) -> Res<(), IO> {
        let (units, hash) = encode(dst)?;
        let source = self.found(src)?;
        if source.is_dir() {
            return Err(Error::InvalidInput);
        }
        let mut old: Option<XEntry<'a, IO>> = None;
        let mut new_there = false;
        for entry in self.iter() {
            let entry = entry?;
            if !entry.is(&units, hash) {
                continue;
            }
            if entry.loc == source.loc {
                return Ok(());
            }
            if entry.is_dir() {
                return Err(Error::InvalidInput);
            }
            let holds = match source.first_cluster() {
                Some(first) => entry.first == first,
                // Once: a second empty one is the old file.
                None => !new_there && entry.first == 0 && entry.len == 0,
            };
            if holds {
                new_there = true;
            } else if old.is_none() {
                old = Some(entry);
            } else {
                return Err(Error::AlreadyExists);
            }
        }
        if !new_there {
            let mut fresh = source.fresh();
            match &old {
                Some(old) => fresh.created = old.created,
                // A mark the caller gave `src` is not left to the file.
                None if fresh.created.1 == MARKED => fresh.created = (NO_TIME, 0),
                None => {}
            }
            self.add(&build(&units, hash, &fresh))?;
        }
        if let Some(old) = &old {
            self.fs.clear(&old.loc)?;
        }
        self.fs.clear(&source.loc)?;
        match &old {
            Some(old) => self.fs.free(old.chain(), old.clusters()),
            None => Ok(()),
        }
    }
}

impl Fresh {
    fn empty(attrs: u16) -> Self {
        Self {
            attrs,
            created: (NO_TIME, 0),
            modified: NO_TIME,
            first: 0,
            len: 0,
            valid: 0,
            contiguous: true,
        }
    }
}

/// The entries of a directory, from a byte of it on.
pub struct XIter<'a, IO: ReadWriteSeek> {
    fs: &'a ExFat<IO>,
    chain: Chain,
    /// The directory's bytes; 0 where the FAT tells its end.
    len: u64,
    at: u64,
    walked: (u32, u32),
    failed: bool,
    done: bool,
}

impl<IO: ReadWriteSeek> Clone for XIter<'_, IO> {
    fn clone(&self) -> Self {
        Self {
            fs: self.fs,
            chain: self.chain,
            len: self.len,
            at: self.at,
            walked: self.walked,
            failed: self.failed,
            done: self.done,
        }
    }
}

impl<'a, IO: ReadWriteSeek> XIter<'a, IO> {
    /// The slot at the iterator's place with the cluster it is in, not
    /// passed; `None` past the directory's clusters.
    fn place(&mut self) -> Res<Option<(u32, [u8; ENTRY])>, IO> {
        if self.failed {
            return Err(Error::CorruptedFileSystem);
        }
        if (self.len != 0 && self.at >= self.len) || self.at >= MAX_DIRECTORY {
            return Ok(None);
        }
        let index = (self.at >> self.fs.cluster_shift()) as u32;
        let Some(cluster) = self.fs.cluster_at(self.chain, &mut self.walked, index)? else {
            return Ok(None);
        };
        let offset = self.at & (self.fs.cluster_size() - 1);
        let mut entry = [0u8; ENTRY];
        self.fs
            .read_at(self.fs.cluster_offset(cluster)? + offset, &mut entry)?;
        Ok(Some((cluster, entry)))
    }

    /// The slot at the iterator's place and its byte in the directory.
    fn slot(&mut self) -> Res<Option<(u64, [u8; ENTRY])>, IO> {
        Ok(self.place()?.map(|(_, entry)| (self.at, entry)))
    }

    fn step(&mut self) -> Res<Option<XEntry<'a, IO>>, IO> {
        loop {
            let Some((cluster, entry)) = self.place()? else {
                return Ok(None);
            };
            if entry[0] == 0 {
                return Ok(None);
            }
            let loc = Loc {
                dir: self.chain.first,
                at: self.at,
                cluster,
                contiguous: self.chain.contiguous,
                count: entry[1].saturating_add(1),
            };
            self.at += ENTRY as u64;
            if entry[0] != ENTRY_FILE {
                continue;
            }
            let Some((set, cut)) = self.fs.load(&loc)? else {
                continue;
            };
            if cut {
                // Finished here, for every other reader of the card; where
                // that fails, the next read finishes it.
                if let Ok(head) = self.fs.entry_pos(&loc, 0) {
                    drop(self.fs.write_at(head, &set.bytes[..ENTRY]));
                }
            }
            self.at += u64::from(loc.count - 1) * ENTRY as u64;
            return XEntry::of(self.fs, loc, &set).map(Some);
        }
    }
}

impl<'a, IO: ReadWriteSeek> Iterator for XIter<'a, IO> {
    type Item = Result<XEntry<'a, IO>, Error<IO::Error>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        match self.step() {
            Ok(Some(entry)) => Some(Ok(entry)),
            Ok(None) => {
                self.done = true;
                None
            }
            Err(e) => {
                self.done = true;
                Some(Err(e))
            }
        }
    }
}

#[cfg(test)]
mod tests;
