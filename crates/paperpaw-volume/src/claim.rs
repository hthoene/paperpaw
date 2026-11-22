// Modified for PaperPaw: added (this file).
//! Clusters a cut would leave taken by no file, said on the disk before they are.
//!
//! A cluster becomes a file's in two writes that cannot be one: its mark in the FAT, and what
//! points at it (the cluster before it in another block of the FAT, or the directory entry).
//! Removing goes the same way backwards: the entry first, then the marks. Cut off between
//! (power lost), the cluster is marked taken and nothing points at it: lost until a computer
//! checks the volume. So the FS information sector (FAT32) says first what is about to be taken
//! or freed, in its reserved bytes (a *claim*), and the next mount ([`FileSystem::settle_claim`])
//! frees what the claim names where nothing points at it.
//!
//! A claim names the first cluster of a chain, what is to point at it (or did), and a hash of
//! the FAT block of that cluster as it is while the chain is taken and unreferenced. It is acted
//! on only where all of this holds, so that a claim that is no longer true frees nothing:
//!
//! - The sector is as this file system wrote it: no count of free clusters and no hint (a
//!   computer that takes clusters writes one of them when it lets go of the volume).
//! - What should point at the chain does not: the cluster before it links elsewhere; the
//!   place of the directory entry is byte for byte as it was before the entry got the cluster
//!   (or holds no entry of the chain any more, for a chain being freed).
//! - The FAT block of the chain's first cluster is byte for byte as the claim says: nothing
//!   took or freed a cluster there since. A device that writes the volume without writing the
//!   sector cannot free or take a cluster that is marked taken and that nothing points at, so
//!   what the claim names is still what it named.
//!
//! A chain is freed a block of the FAT at a time, each block written once; the claim names
//! where the chain goes on in another block too, with that block's hash, and is written anew
//! before each block: what a cut leaves of the chain is found from it.
//!
//! One claim is on the disk at most, the one of the change made last. Nothing can make it true
//! again for clusters in use: a claim of freed clusters is taken away before the next cluster
//! is taken; a claim that names the place of an entry is taken away before an entry that holds
//! no cluster is written, and an entry removed at that place never reads as the place did
//! before (`unlike_claim`). The count kept by `keep_free_count` overwrites a claim.
//!
//! FAT12 and FAT16 have no such sector: there a cut between two such writes still leaves the
//! clusters taken.

use super::*;
use crate::table::{
    BLOCK, FatValue, block_of, free_in_block, hash_block, leaves_block, read_block, read_fat,
    set_in_block, write_fat,
};

/// What is to point at a chain, or did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Referrer {
    /// Not known: nothing is claimed.
    Unknown,
    /// The cluster before it in its chain.
    Link(u32),
    /// The directory entry at this place on the disk, which holds no cluster yet or is not
    /// written yet.
    NewEntry(u64),
    /// The directory entry at this place on the disk, about to be removed.
    Entry(u64),
    /// Nothing any more.
    Gone,
}

const MAGIC: &[u8; 4] = b"PPCL";
/// Bytes of a claim in the sector's reserved bytes.
const RECORD: usize = 48;
const UNKNOWN: u32 = 0xFFFF_FFFF;

/// A claim as the sector holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Claim {
    kind: u32,
    /// The first cluster of the chain.
    start: u32,
    /// Where the chain goes on outside the FAT block of `start` (0: nowhere), for a chain being
    /// freed a block at a time.
    then: u32,
    /// The referrer: a cluster or a place on the disk.
    at: u64,
    /// The hash of the 32 bytes at `at` as they are before the entry there gets the cluster
    /// (`NewEntry`).
    entry: u32,
    /// The hashes of the FAT blocks of `start` and of `then` while they are taken.
    block: u32,
    then_block: u32,
}

const LINK: u32 = 1;
const NEW_ENTRY: u32 = 2;
const ENTRY: u32 = 4;
const GONE: u32 = 5;
/// A claim of an earlier mount that was not looked at (`settle_claim`).
const STALE: u32 = 6;
/// As `LINK`, of a chain about to be cut off the cluster before it and freed.
const CUT: u32 = 7;

impl Claim {
    /// Whether it is of clusters being freed (or may be): they are free in the end, and the
    /// claim must be gone before one of them is taken again.
    fn frees(&self) -> bool {
        matches!(self.kind, CUT | ENTRY | GONE | STALE)
    }
}

fn fnv(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811C_9DC5_u32, |hash, &byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// The first cluster a directory entry's 32 bytes hold (FAT32).
fn first_cluster(entry: &[u8; 32]) -> u32 {
    u32::from(u16::from_le_bytes([entry[26], entry[27]]))
        | u32::from(u16::from_le_bytes([entry[20], entry[21]])) << 16
}

/// Whether the 32 bytes are an entry of a file or a directory that is there.
fn is_alive(entry: &[u8; 32]) -> bool {
    !matches!(entry[0], 0 | 0xE5) && entry[11] & 0x0F != 0x0F && entry[11] & 0x08 == 0
}

impl<IO: ReadWriteSeek, TP, OCC> FileSystem<IO, TP, OCC> {
    fn claims(&self) -> bool {
        self.fat_type == FatType::Fat32
    }

    /// Writes the FS information sector: no count, and `claim` with no hint, or the hint and no
    /// claim. The disk's position is left where it was.
    fn write_claim(&self, claim: Option<Claim>) -> Result<(), Error<IO::Error>> {
        let mut fs_info = self.fs_info.borrow_mut();
        fs_info.free_cluster_count = None;
        let mut reserved = [0_u8; RECORD];
        if let Some(claim) = claim {
            reserved[..4].copy_from_slice(MAGIC);
            reserved[4..8].copy_from_slice(&self.total_clusters.to_le_bytes());
            reserved[8..12]
                .copy_from_slice(&fs_info.next_free_cluster.unwrap_or(UNKNOWN).to_le_bytes());
            reserved[12..16].copy_from_slice(&claim.kind.to_le_bytes());
            reserved[16..20].copy_from_slice(&claim.start.to_le_bytes());
            reserved[20..24].copy_from_slice(&claim.then.to_le_bytes());
            reserved[24..32].copy_from_slice(&claim.at.to_le_bytes());
            reserved[32..36].copy_from_slice(&claim.entry.to_le_bytes());
            reserved[36..40].copy_from_slice(&claim.block.to_le_bytes());
            reserved[40..44].copy_from_slice(&claim.then_block.to_le_bytes());
            let hash = fnv(&reserved[..44]);
            reserved[44..48].copy_from_slice(&hash.to_le_bytes());
        }
        let mut disk = self.disk.borrow_mut();
        let was = disk.seek(SeekFrom::Current(0))?;
        disk.seek(SeekFrom::Start(
            self.offset_from_sector(u32::from(self.bpb.fs_info_sector)),
        ))?;
        let hint = fs_info.next_free_cluster;
        if claim.is_some() {
            fs_info.next_free_cluster = None;
        }
        let written = fs_info.serialize_noted(&mut *disk, &reserved);
        fs_info.next_free_cluster = hint;
        written?;
        fs_info.dirty = false;
        disk.seek(SeekFrom::Start(was))?;
        self.count_kept.set(false);
        self.claim.set(claim);
        Ok(())
    }

    /// Takes the claim on the disk away, where there is one.
    pub(crate) fn drop_claim(&self) -> Result<(), Error<IO::Error>> {
        if self.claim.get().is_some() {
            self.write_claim(None)?;
        }
        Ok(())
    }

    /// Before a cluster is taken with no claim of its own: a claim of clusters freed would
    /// name it once it is taken again.
    pub(crate) fn drop_free_claim(&self) -> Result<(), Error<IO::Error>> {
        if self.claim.get().is_some_and(|claim| claim.frees()) {
            self.write_claim(None)?;
        }
        Ok(())
    }

    /// Before the entries of the chain from `cluster` are removed and its clusters left (a
    /// move, an entry that shares its data): a claim of that chain, left by a removal that
    /// failed, would take the removal for its own.
    pub(crate) fn drop_free_claim_of(&self, cluster: Option<u32>) -> Result<(), Error<IO::Error>> {
        match self.claim.get() {
            Some(claim)
                if claim.frees() && (claim.kind == STALE || Some(claim.start) == cluster) =>
            {
                self.write_claim(None)
            }
            _ => Ok(()),
        }
    }

    /// After the entry at `at` was marked removed: where a claim names that place and the
    /// removed entry reads, byte for byte, as the place did before the claim's entry was written
    /// (the same name removed there twice, on a device without a clock), one byte of it that
    /// means nothing in a removed entry is changed. The claim then cannot take a cluster that
    /// got its entry for one that never did.
    pub(crate) fn unlike_claim(&self, at: u64) -> Result<(), Error<IO::Error>> {
        let Some(claim) = self
            .claim
            .get()
            .filter(|claim| claim.kind == NEW_ENTRY && claim.at == at)
        else {
            return Ok(());
        };
        let entry = self.entry_at(at)?;
        if fnv(&entry) == claim.entry {
            let mut disk = self.disk.borrow_mut();
            let was = disk.seek(SeekFrom::Current(0))?;
            disk.seek(SeekFrom::Start(at + 12))?;
            disk.write_all(&[entry[12] ^ 1])?;
            disk.seek(SeekFrom::Start(was))?;
        }
        Ok(())
    }

    /// Before an entry that holds no cluster is written: it may be, byte for byte, the entry a
    /// claim of a new file's first cluster names.
    pub(crate) fn drop_entry_claim(&self) -> Result<(), Error<IO::Error>> {
        if self
            .claim
            .get()
            .is_some_and(|claim| matches!(claim.kind, NEW_ENTRY | STALE))
        {
            self.write_claim(None)?;
        }
        Ok(())
    }

    fn fat_block_hash(&self, cluster: u32) -> Result<u32, Error<IO::Error>> {
        hash_block(&mut self.fat_slice(), self.fat_type, cluster, &[])
    }

    fn entry_at(&self, at: u64) -> Result<[u8; 32], Error<IO::Error>> {
        let mut entry = [0_u8; 32];
        let mut disk = self.disk.borrow_mut();
        let was = disk.seek(SeekFrom::Current(0))?;
        disk.seek(SeekFrom::Start(at))?;
        let read = disk.read_exact(&mut entry);
        disk.seek(SeekFrom::Start(was))?;
        read?;
        Ok(entry)
    }

    /// Before clusters from `first` are marked taken, where the mark and what points at them
    /// (`referrer`) are not one write: says so on the disk. `entries` are the bytes about to be
    /// written at the entry of `first`, all in one block of the FAT, which `fat` reads.
    pub(crate) fn claim_taking<S>(
        &self,
        fat: &mut S,
        first: u32,
        entries: &[u8],
        referrer: Referrer,
    ) -> Result<(), Error<IO::Error>>
    where
        S: Read + Seek,
        Error<IO::Error>: From<S::Error>,
    {
        let (kind, at) = match referrer {
            Referrer::Link(cluster) => (LINK, u64::from(cluster)),
            Referrer::NewEntry(at) => (NEW_ENTRY, at),
            _ => (0, 0),
        };
        if !self.claims() || kind == 0 || entries.is_empty() {
            // Nothing to say for it; what is said of freed clusters must go all the same.
            return self.drop_free_claim();
        }
        let block = hash_block(fat, self.fat_type, first, entries)?;
        let entry = match referrer {
            Referrer::NewEntry(at) => fnv(&self.entry_at(at)?),
            _ => 0,
        };
        self.write_claim(Some(Claim {
            kind,
            start: first,
            then: 0,
            at,
            entry,
            block,
            then_block: 0,
        }))
    }

    /// Before what points at the chain from `start` (`referrer`) is taken away and the chain
    /// freed (`free_claimed`): says so on the disk.
    pub(crate) fn claim_freeing(
        &self,
        start: u32,
        referrer: Referrer,
    ) -> Result<(), Error<IO::Error>> {
        if !self.claims() {
            return Ok(());
        }
        let (kind, at) = match referrer {
            Referrer::Link(cluster) => (CUT, u64::from(cluster)),
            Referrer::Entry(at) => (ENTRY, at),
            _ => (GONE, 0),
        };
        // A link out of the volume (a damaged chain) is not followed: freeing fails there, as
        // it did, after what comes before it is freed.
        let then = leaves_block(&mut self.fat_slice(), self.fat_type, start)?
            .filter(|&next| self.is_cluster(next));
        let then_block = match then {
            Some(then) => self.fat_block_hash(then)?,
            None => 0,
        };
        self.write_claim(Some(Claim {
            kind,
            start,
            then: then.unwrap_or(0),
            at,
            entry: 0,
            block: self.fat_block_hash(start)?,
            then_block,
        }))
    }

    /// Frees the chain from `start`, a block of the FAT at a time (each written once), saying
    /// first on the disk where it goes on. `claimed`: `claim_freeing` said so for the first
    /// block already, and what pointed at the chain is gone since.
    ///
    /// A chain that links out of the volume (or an entry's first cluster that is none of its
    /// clusters) is freed up to there, and freeing fails there: entry 1 of the FAT, which holds
    /// the volume's clean-shutdown bits, is no cluster to free.
    pub(crate) fn free_claimed(
        &self,
        mut start: u32,
        mut claimed: bool,
    ) -> Result<(), Error<IO::Error>> {
        loop {
            if !self.is_cluster(start) {
                return Err(Error::CorruptedFileSystem);
            }
            if self.fat_type == FatType::Fat12 {
                return self.free_chain_one_by_one(start);
            }
            if !claimed {
                self.claim_freeing(start, Referrer::Gone)?;
            }
            claimed = false;
            match self.free_block(start, None)? {
                Some(next) => start = next,
                None => return Ok(()),
            }
        }
    }

    /// Frees the clusters of the chain from `start` whose entries lie in its block of the FAT,
    /// and makes `end` (in the same block) the end of its chain, in one write. Where the chain
    /// goes on in another block. The block is on the stack here only, not below what says the
    /// claim.
    #[inline(never)]
    fn free_block(&self, start: u32, end: Option<u32>) -> Result<Option<u32>, Error<IO::Error>> {
        let (freed, next) = {
            let mut fat = self.fat_slice();
            let mut block = [0_u8; BLOCK];
            read_block(&mut fat, self.fat_type, start, &mut block)?;
            let (freed, next) = free_in_block(
                &mut fat,
                self.fat_type,
                &mut block,
                start,
                self.total_clusters,
            )?;
            if let Some(end) = end {
                set_in_block(&mut block, self.fat_type, end, FatValue::EndOfChain);
            }
            if freed > 0 || end.is_some() {
                fat.seek(SeekFrom::Start(block_of(self.fat_type, start)))?;
                fat.write_all(&block)?;
            }
            (freed, next)
        };
        // Counted once written: a write that failed part of the way leaves the count low.
        self.note_freed(freed);
        self.fs_info
            .borrow_mut()
            .map_free_clusters(|n| n.saturating_add(freed));
        Ok(next.filter(|_| freed > 0))
    }

    /// `keep` made the end of its chain, and the chain from `first`, which it links to, freed.
    pub(super) fn truncate_claimed(&self, keep: u32, first: u32) -> Result<(), Error<IO::Error>> {
        let same = block_of(self.fat_type, keep) == block_of(self.fat_type, first);
        let goes_on = leaves_block(&mut self.fat_slice(), self.fat_type, first)?.is_some();
        if !same || goes_on {
            self.claim_freeing(first, Referrer::Link(keep))?;
        }
        if same {
            // The end and what is freed in its block are one write: a cut leaves both or
            // neither, and what goes on in another block is said.
            match self.free_block(first, Some(keep))? {
                Some(next) => self.free_claimed(next, false),
                None => Ok(()),
            }
        } else {
            write_fat(
                &mut self.fat_slice(),
                self.fat_type,
                keep,
                FatValue::EndOfChain,
            )?;
            self.free_claimed(first, true)
        }
    }

    pub(crate) fn is_cluster(&self, cluster: u32) -> bool {
        (RESERVED_FAT_ENTRIES..self.total_clusters + RESERVED_FAT_ENTRIES).contains(&cluster)
    }

    /// The claim the sector holds, where it is one and the sector is as this file system wrote
    /// it with one; the hint that was followed then.
    fn read_claim(&self) -> Result<Option<(Claim, u32)>, Error<IO::Error>> {
        let mut reserved = [0_u8; RECORD];
        let Some((free, next)) = self.kept_free_count(&mut reserved)? else {
            return Ok(None);
        };
        let whole = &reserved[..4] == MAGIC && word(&reserved, 44) == fnv(&reserved[..44]);
        if !whole || free != UNKNOWN || next != UNKNOWN || word(&reserved, 4) != self.total_clusters
        {
            return Ok(None);
        }
        let claim = Claim {
            kind: word(&reserved, 12),
            start: word(&reserved, 16),
            then: word(&reserved, 20),
            at: u64::from(word(&reserved, 24)) | u64::from(word(&reserved, 28)) << 32,
            entry: word(&reserved, 32),
            block: word(&reserved, 36),
            then_block: word(&reserved, 40),
        };
        Ok(Some((claim, word(&reserved, 8))))
    }

    /// Whether what `claim` says should point at its chain does not.
    fn is_unreferenced(&self, claim: &Claim) -> Result<bool, Error<IO::Error>> {
        Ok(match claim.kind {
            LINK | CUT => match u32::try_from(claim.at) {
                Ok(cluster) if self.is_cluster(cluster) => {
                    read_fat(&mut self.fat_slice(), self.fat_type, cluster)?
                        != FatValue::Data(claim.start)
                }
                _ => false,
            },
            NEW_ENTRY => fnv(&self.entry_at(claim.at)?) == claim.entry,
            ENTRY => {
                let entry = self.entry_at(claim.at)?;
                !(is_alive(&entry) && first_cluster(&entry) == claim.start)
            }
            GONE => true,
            _ => false,
        })
    }

    /// After a mount, before anything is changed: frees the clusters a cut left taken by no
    /// file, where the FS information sector holds a claim of them that still holds (see the
    /// module documentation), and takes the claim away. How many clusters were freed;
    /// they are counted in `cluster_changes`. Nothing is read where the mount found no claim.
    ///
    /// # Errors
    ///
    /// `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn settle_claim(&self) -> Result<u32, Error<IO::Error>> {
        if !self.claims() || self.claim.get().is_none() {
            return Ok(0);
        }
        let Some((claim, hint)) = self.read_claim()? else {
            // Not one, or the sector was written by another since: the first change of the FAT
            // overwrites it.
            self.claim.set(None);
            return Ok(0);
        };
        {
            let mut fs_info = self.fs_info.borrow_mut();
            if fs_info.next_free_cluster.is_none() && hint != UNKNOWN {
                fs_info.next_free_cluster = Some(hint);
                fs_info.validate_and_fix(self.total_clusters);
            }
        }
        let before = self.clusters_freed.get();
        if self.is_cluster(claim.start) && self.is_unreferenced(&claim)? {
            if self.fat_block_hash(claim.start)? == claim.block {
                self.free_claimed(claim.start, false)?;
            } else if self.is_cluster(claim.then)
                && self.fat_block_hash(claim.then)? == claim.then_block
            {
                self.free_claimed(claim.then, false)?;
            }
        }
        self.write_claim(None)?;
        Ok(u32::try_from(self.clusters_freed.get().wrapping_sub(before)).unwrap_or(u32::MAX))
    }

    /// Whether the mount found a claim in the FS information sector that `settle_claim` has
    /// not looked at yet.
    pub(super) fn stale_claim(found: bool) -> Option<Claim> {
        found.then_some(Claim {
            kind: STALE,
            start: 0,
            then: 0,
            at: 0,
            entry: 0,
            block: 0,
            then_block: 0,
        })
    }
}
