//! Modified for PaperPaw: bounded bootstrap prototype for an ordinary journal file.
//!
//! Not enabled automatically. Publication uses one device block and requires
//! separate qualification of torn-block behaviour before release.
use super::*;
use crate::exfat::intent::{self, Change, Identity, Operation};

/// Reserved short root name; it fits one File/Stream/Name entry set.
pub const INTENT_FILE: &str = "PP$INTENT";
/// Fixed file length: immutable identity manifest and two record slots.
pub const INTENT_FILE_BYTES: u64 = 8192;
/// Transaction slots start after the immutable manifest.
pub const INTENT_SLOTS_AT: u64 = intent::RECORD_BYTES as u64;

/// Verified journal extent and directory entry location.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JournalLocation {
    /// Volume and extent identity validated against the manifest.
    pub identity: Identity,
    /// Primary directory entry of the journal itself.
    pub entry: u64,
}

io_transform! {
impl<D: BlockDevice> ExFatFs<D> {
    /// Creates or verifies the journal, without enabling metadata hooks.
    /// Refuses root growth, fragmented space and unknown existing files.
    /// A failed unpublished allocation is left reserved, never guessed free.
    /// `epoch` must be nonzero and fresh for a newly initialized journal.
    pub async fn bootstrap_intent(&mut self, epoch: u64) -> FsResult<JournalLocation, D::Error> {
        if epoch == 0 { return Err(ErrorKind::InvalidInput.into()); }
        self.prepare().await?;
        let units = NameUnits::encode(INTENT_FILE)?;
        let mut found = Located::new();
        if self.find(self.root_start(), &units, &mut found).await? {
            let location = self.verify_intent(&found.set).await?;
            // A verified publication ends an uncertain attempt of this mount;
            // `sync` still flushes it all before it clears `VolumeDirty`.
            self.intent_uncertain = false;
            return Ok(location);
        }
        // The dirty flag is flushed before allocation. Without a verified
        // published journal, a dirty volume may hold an interrupted attempt.
        // Never allocate another extent on each retry or guess which to free.
        // An attempt that failed during this mount latched the same refusal,
        // and `sync` keeps the flag set for the next mount.
        if self.intent_uncertain || self.vol.flags() & raw::VOLUME_DIRTY != 0 {
            return Err(hadris_fs::Error::new(ErrorKind::Corrupt,
                "journal initialization uncertain; volume check required before retry"));
        }
        let (at, slot, end) = self.intent_slots().await?;
        let count = INTENT_FILE_BYTES.div_ceil(self.vol.geometry().cluster_size()) as u32;
        let first = self.intent_free_run(count).await?;
        let alloc = Alloc { first, contiguous: true };
        let start = self.cluster_at(first)?;
        let mut primary = [0; ENTRY_SIZE];
        primary[4..6].copy_from_slice(&(raw::ATTR_HIDDEN | raw::ATTR_SYSTEM).to_le_bytes());
        let now = self.now();
        stamp(&mut primary, CREATED, now);
        stamp(&mut primary, MODIFIED, now);
        stamp(&mut primary, ACCESSED, now);
        let mut stream = [0; ENTRY_SIZE];
        set_stream(&mut stream, alloc, INTENT_FILE_BYTES, INTENT_FILE_BYTES);
        let mut entries = [[0; ENTRY_SIZE]; MAX_SET];
        let hash = self.name_hash(&units).await?;
        let n = build_set(&primary, &stream, &units, hash, &[], &mut entries)?;
        if n != 3 { return Err(ErrorKind::Corrupt.into()); }
        let identity = self.intent_identity(start, epoch);
        let mut manifest = alloc::vec::Vec::new();
        manifest.try_reserve_exact(intent::RECORD_BYTES).map_err(|_| ErrorKind::LimitExceeded)?;
        manifest.resize(intent::RECORD_BYTES, 0);
        let changes = [
            Change { offset: at[0], before: &[0; 32], after: &entries[0] },
            Change { offset: at[1], before: &[0; 32], after: &entries[1] },
            Change { offset: at[2], before: &[0; 32], after: &entries[2] },
        ];
        intent::encode(&mut manifest, identity, 1, Operation::Insert, &changes).map_err(|_| ErrorKind::Corrupt)?;
        intent::applied_header(&mut manifest, identity).map_err(|_| ErrorKind::Corrupt)?;
        // No filesystem handle creates or extends this file. Allocation and
        // initialized data are durable before any directory entry names it.
        self.writable()?;
        // Latched before the first write; only a completed publication below
        // or a later verification clears it.
        self.intent_uncertain = true;
        exio::set_bits(&mut self.dev, &mut self.block, &mut self.vol, first, count, ClusterState::Used).await?;
        self.put(start, None, INTENT_FILE_BYTES).await?;
        self.put_bytes(start, &manifest).await?;
        self.flush_device().await?;
        if let Some(end) = end {
            let mut walk = walk(self.root_start());
            for skipped in end..slot {
                let offset = self.slot_offset(&mut walk, skipped).await?.ok_or(ErrorKind::Corrupt)?;
                self.put_bytes(offset, &[1]).await?;
            }
            self.flush_device().await?;
        }
        self.put_entries(&at, &entries[..3]).await?;
        self.flush_device().await?;
        self.intent_uncertain = false;
        Ok(JournalLocation { identity, entry: at[0] })
    }

    /// Whether a journal bootstrap of this mount failed before it verifiably
    /// published. Until it is verified, `sync` leaves `VolumeDirty` set and
    /// no further bootstrap allocates.
    pub fn intent_uncertain(&self) -> bool {
        self.intent_uncertain
    }

    fn intent_identity(&self, start: u64, epoch: u64) -> Identity {
        let geo = self.vol.geometry();
        Identity {
            serial: geo.volume_serial(), volume_bytes: geo.volume_len(),
            heap_start: geo.heap_start(),
            heap_end: geo.heap_start() + u64::from(geo.cluster_count()) * geo.cluster_size(),
            journal_start: start, journal_len: INTENT_FILE_BYTES, epoch,
        }
    }

    async fn verify_intent(&mut self, set: &Set) -> FsResult<JournalLocation, D::Error> {
        if set.count != 3 || set.is_dir() || !set.alloc().contiguous
            || set.data_len() != INTENT_FILE_BYTES || le64(&set.raw[1], 8) != INTENT_FILE_BYTES
            || set.attributes() & (raw::ATTR_HIDDEN | raw::ATTR_SYSTEM) != raw::ATTR_HIDDEN | raw::ATTR_SYSTEM {
            return Err(ErrorKind::Unsupported.into());
        }
        let start = self.cluster_at(set.first())?;
        let count = INTENT_FILE_BYTES.div_ceil(self.vol.geometry().cluster_size()) as u32;
        for i in 0..count {
            if !self.bit(set.first().checked_add(i).ok_or(ErrorKind::Corrupt)?).await? {
                return Err(ErrorKind::Corrupt.into());
            }
        }
        let mut manifest = alloc::vec::Vec::new();
        manifest.try_reserve_exact(intent::RECORD_BYTES).map_err(|_| ErrorKind::LimitExceeded)?;
        manifest.resize(intent::RECORD_BYTES, 0);
        self.get_bytes(start, &mut manifest).await?;
        let identity = self.intent_identity(start, le64(&manifest, 72));
        let record = intent::decode(&manifest, identity).map_err(|_| ErrorKind::Corrupt)?
            .ok_or(ErrorKind::Corrupt)?;
        if !record.applied || record.operation != Operation::Insert || record.len() != 3 {
            return Err(ErrorKind::Corrupt.into());
        }
        for i in 0..3 {
            let change = record.change(i).ok_or(ErrorKind::Corrupt)?;
            if change.offset != set.at[i] || change.after != &set.raw[i] {
                return Err(ErrorKind::Corrupt.into());
            }
        }
        Ok(JournalLocation { identity, entry: set.at[0] })
    }

    /// Finds existing free slots without extending or freeing a directory.
    /// Scans the whole root up to its end marker first: a damaged
    /// reserved-name set anywhere refuses, also after a usable free run.
    async fn intent_slots(&mut self) -> FsResult<([u64; 3], u32, Option<u32>), D::Error> {
        let mut walk = walk(self.root_start());
        let mut slot = 0;
        let mut end = None;
        let mut at = [0; 3];
        let mut run = 0;
        let mut chosen = None;
        while let Some(offset) = self.slot_offset(&mut walk, slot).await? {
            if end.is_some() && chosen.is_some() { break; }
            let mut entry = [0; ENTRY_SIZE];
            if end.is_none() { self.get_bytes(offset, &mut entry).await?; }
            if entry[0] == raw::ENTRY_END { end.get_or_insert(slot); }
            if entry[0] == raw::ENTRY_FILE {
                // A damaged reserved-name set is evidence, not free space
                // or permission to create a second journal with that name.
                if let Some(name_at) = self.slot_offset(&mut walk, slot + 2).await? {
                    let mut named = [0; ENTRY_SIZE];
                    self.get_bytes(name_at, &mut named).await?;
                    if named[0] == raw::ENTRY_NAME
                        && INTENT_FILE.bytes().enumerate().all(|(i, byte)| le16(&named, 2 + i * 2) == u16::from(byte)) {
                        return Err(ErrorKind::Corrupt.into());
                    }
                }
            }
            if entry[0] & raw::IN_USE == 0 {
                if chosen.is_none() {
                    if run > 0 && (offset != at[run-1] + 32 || offset / 512 != at[0] / 512) { run = 0; }
                    at[run] = offset;
                    run += 1;
                    if run == 3 { chosen = Some(slot - 2); }
                }
            } else {
                run = 0;
                // Preserve an ambiguous primary's entire claimed set.
                if entry[0] == raw::ENTRY_FILE || (entry[0] & raw::CATEGORY_SECONDARY == 0 && entry[0] & raw::IMPORTANCE_BENIGN != 0) {
                    slot += u32::from(entry[1]);
                }
            }
            slot += 1;
        }
        match chosen {
            // Skipped end-marker slots only matter before the chosen run.
            Some(chosen) => Ok((at, chosen, end.filter(|&end| end < chosen))),
            None => Err(ErrorKind::NoSpace.into()),
        }
    }

    async fn intent_free_run(&mut self, count: u32) -> FsResult<u32, D::Error> {
        let mut length = 0;
        for cluster in 2..=self.vol.geometry().max_cluster() {
            length = if self.bit(cluster).await? { 0 } else { length + 1 };
            if length == count { return Ok(cluster + 1 - count); }
        }
        Err(ErrorKind::NoSpace.into())
    }
}
}
