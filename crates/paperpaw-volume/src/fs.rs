use alloc::string::String;
use core::borrow::BorrowMut;
use core::cell::{Cell, RefCell};
use core::convert::TryFrom;
use core::fmt::Debug;
use core::marker::PhantomData;

use crate::boot_sector::{BiosParameterBlock, BootSector};
use crate::dir::{Dir, DirRawStream};
use crate::dir_entry::{SFN_PADDING, SFN_SIZE};
use crate::error::{Error, IoError};
use crate::file::File;
use crate::io::{self, IoBase, Read, ReadLeExt, Seek, SeekFrom, Write, WriteLeExt};
use crate::table::{
    ClusterIterator, FatValue, RESERVED_FAT_ENTRIES, count_free_clusters, read_fat, read_fat_flags,
    write_fat,
};
use crate::time::{DefaultTimeProvider, TimeProvider};

// FAT implementation based on:
//   http://wiki.osdev.org/FAT
//   https://www.win.tue.nl/~aeb/linux/fs/fat/fat-1.html

/// A type of FAT filesystem.
///
/// `FatType` values are based on the size of File Allocation Table entry.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum FatType {
    /// 12 bits per FAT entry
    Fat12,
    /// 16 bits per FAT entry
    Fat16,
    /// 32 bits per FAT entry
    Fat32,
}

impl FatType {
    /// Whether entries are 32 bits wide: always, where FAT12 and FAT16 are not built in.
    #[inline]
    pub(crate) fn wide(self) -> bool {
        !cfg!(feature = "fat16") || self == FatType::Fat32
    }

    const FAT16_MIN_CLUSTERS: u32 = 4085;
    const FAT32_MIN_CLUSTERS: u32 = 65525;
    const FAT32_MAX_CLUSTERS: u32 = 0x0FFF_FFF4;

    pub(crate) fn from_clusters(total_clusters: u32) -> Self {
        if total_clusters < Self::FAT16_MIN_CLUSTERS {
            FatType::Fat12
        } else if total_clusters < Self::FAT32_MIN_CLUSTERS {
            FatType::Fat16
        } else {
            FatType::Fat32
        }
    }

    pub(crate) fn bits_per_fat_entry(self) -> u32 {
        match self {
            FatType::Fat12 => 12,
            FatType::Fat16 => 16,
            FatType::Fat32 => 32,
        }
    }

    pub(crate) fn min_clusters(self) -> u32 {
        match self {
            FatType::Fat12 => 0,
            FatType::Fat16 => Self::FAT16_MIN_CLUSTERS,
            FatType::Fat32 => Self::FAT32_MIN_CLUSTERS,
        }
    }

    pub(crate) fn max_clusters(self) -> u32 {
        match self {
            FatType::Fat12 => Self::FAT16_MIN_CLUSTERS - 1,
            FatType::Fat16 => Self::FAT32_MIN_CLUSTERS - 1,
            FatType::Fat32 => Self::FAT32_MAX_CLUSTERS,
        }
    }
}

/// A FAT volume status flags retrived from the Boot Sector and the allocation table second entry.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct FsStatusFlags {
    pub(crate) dirty: bool,
    pub(crate) io_error: bool,
}

impl FsStatusFlags {
    /// Checks if the volume is marked as dirty.
    ///
    /// Dirty flag means volume has been suddenly ejected from filesystem without unmounting.
    #[must_use]
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// Checks if the volume has the IO Error flag active.
    #[must_use]
    pub fn io_error(&self) -> bool {
        self.io_error
    }

    fn encode(self) -> u8 {
        let mut res = 0_u8;
        if self.dirty {
            res |= 1;
        }
        if self.io_error {
            res |= 2;
        }
        res
    }

    pub(crate) fn decode(flags: u8) -> Self {
        Self {
            dirty: flags & 1 != 0,
            io_error: flags & 2 != 0,
        }
    }
}

/// A sum of `Read` and `Seek` traits.
pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// A sum of `Read`, `Write` and `Seek` traits.
pub trait ReadWriteSeek: Read + Write + Seek {}
impl<T: Read + Write + Seek> ReadWriteSeek for T {}

#[derive(Clone, Default, Debug)]
pub(crate) struct FsInfoSector {
    pub(crate) free_cluster_count: Option<u32>,
    pub(crate) next_free_cluster: Option<u32>,
    pub(crate) dirty: bool,
    // Modified for PaperPaw: the reserved bytes hold a claim (see `claim`), as read.
    pub(crate) claimed: bool,
}

impl FsInfoSector {
    const LEAD_SIG: u32 = 0x4161_5252;
    const STRUC_SIG: u32 = 0x6141_7272;
    const TRAIL_SIG: u32 = 0xAA55_0000;

    fn deserialize<R: Read>(rdr: &mut R) -> Result<Self, Error<R::Error>> {
        let lead_sig = rdr.read_u32_le()?;
        if lead_sig != Self::LEAD_SIG {
            error!("invalid lead_sig in FsInfo sector: {}", lead_sig);
            return Err(Error::CorruptedFileSystem);
        }
        let mut reserved = [0_u8; 480];
        rdr.read_exact(&mut reserved)?;
        let struc_sig = rdr.read_u32_le()?;
        if struc_sig != Self::STRUC_SIG {
            error!("invalid struc_sig in FsInfo sector: {}", struc_sig);
            return Err(Error::CorruptedFileSystem);
        }
        let free_cluster_count = match rdr.read_u32_le()? {
            0xFFFF_FFFF => None,
            // Note: value is validated in FileSystem::new function using values from BPB
            n => Some(n),
        };
        let next_free_cluster = match rdr.read_u32_le()? {
            0xFFFF_FFFF => None,
            0 | 1 => {
                warn!("invalid next_free_cluster in FsInfo sector (values 0 and 1 are reserved)");
                None
            }
            // Note: other values are validated in FileSystem::new function using values from BPB
            n => Some(n),
        };
        let mut reserved2 = [0_u8; 12];
        rdr.read_exact(&mut reserved2)?;
        let trail_sig = rdr.read_u32_le()?;
        if trail_sig != Self::TRAIL_SIG {
            error!("invalid trail_sig in FsInfo sector: {}", trail_sig);
            return Err(Error::CorruptedFileSystem);
        }
        Ok(Self {
            free_cluster_count,
            next_free_cluster,
            dirty: false,
            claimed: reserved.starts_with(b"PPCL"),
        })
    }

    pub(crate) fn serialize<W: Write>(&self, wrt: &mut W) -> Result<(), Error<W::Error>> {
        self.serialize_noted(wrt, &[])
    }

    // Modified for PaperPaw: added. As `serialize`, with `note` in the first of the reserved bytes
    // (see `FileSystem::keep_free_count`); the rest of them zero, as `serialize` writes them all.
    fn serialize_noted<W: Write>(&self, wrt: &mut W, note: &[u8]) -> Result<(), Error<W::Error>> {
        wrt.write_u32_le(Self::LEAD_SIG)?;
        let mut reserved = [0_u8; 480];
        let noted = note.len().min(reserved.len());
        reserved[..noted].copy_from_slice(&note[..noted]);
        wrt.write_all(&reserved)?;
        wrt.write_u32_le(Self::STRUC_SIG)?;
        wrt.write_u32_le(self.free_cluster_count.unwrap_or(0xFFFF_FFFF))?;
        wrt.write_u32_le(self.next_free_cluster.unwrap_or(0xFFFF_FFFF))?;
        let reserved2 = [0_u8; 12];
        wrt.write_all(&reserved2)?;
        wrt.write_u32_le(Self::TRAIL_SIG)?;
        Ok(())
    }

    fn validate_and_fix(&mut self, total_clusters: u32) {
        let max_valid_cluster_number = total_clusters + RESERVED_FAT_ENTRIES;
        if let Some(n) = self.free_cluster_count {
            if n > total_clusters {
                warn!(
                    "invalid free_cluster_count ({}) in fs_info exceeds total cluster count ({})",
                    n, total_clusters
                );
                self.free_cluster_count = None;
            }
        }
        if let Some(n) = self.next_free_cluster {
            if n > max_valid_cluster_number {
                warn!(
                    "invalid free_cluster_count ({}) in fs_info exceeds maximum cluster number ({})",
                    n, max_valid_cluster_number
                );
                self.next_free_cluster = None;
            }
        }
    }

    fn map_free_clusters(&mut self, map_fn: impl Fn(u32) -> u32) {
        if let Some(n) = self.free_cluster_count {
            self.free_cluster_count = Some(map_fn(n));
            self.dirty = true;
        }
    }

    fn set_next_free_cluster(&mut self, cluster: u32) {
        self.next_free_cluster = Some(cluster);
        self.dirty = true;
    }

    fn set_free_cluster_count(&mut self, free_cluster_count: u32) {
        self.free_cluster_count = Some(free_cluster_count);
        self.dirty = true;
    }
}

/// A FAT filesystem mount options.
///
/// Options are specified as an argument for `FileSystem::new` method.
#[derive(Copy, Clone, Debug, Default)]
pub struct FsOptions<TP, OCC> {
    pub(crate) update_accessed_date: bool,
    pub(crate) oem_cp_converter: OCC,
    pub(crate) time_provider: TP,
    pub(crate) strict: bool,
}

impl FsOptions<DefaultTimeProvider, LossyOemCpConverter> {
    /// Creates a `FsOptions` struct with default options.
    #[must_use]
    pub fn new() -> Self {
        Self {
            update_accessed_date: false,
            oem_cp_converter: LossyOemCpConverter::new(),
            time_provider: DefaultTimeProvider::new(),
            strict: true,
        }
    }
}

impl<TP: TimeProvider, OCC: OemCpConverter> FsOptions<TP, OCC> {
    /// If enabled accessed date field in directory entry is updated when reading or writing a file.
    #[must_use]
    pub fn update_accessed_date(mut self, enabled: bool) -> Self {
        self.update_accessed_date = enabled;
        self
    }

    /// Changes default OEM code page encoder-decoder.
    pub fn oem_cp_converter<OCC2: OemCpConverter>(
        self,
        oem_cp_converter: OCC2,
    ) -> FsOptions<TP, OCC2> {
        FsOptions::<TP, OCC2> {
            update_accessed_date: self.update_accessed_date,
            oem_cp_converter,
            time_provider: self.time_provider,
            strict: self.strict,
        }
    }

    /// Changes default time provider.
    pub fn time_provider<TP2: TimeProvider>(self, time_provider: TP2) -> FsOptions<TP2, OCC> {
        FsOptions::<TP2, OCC> {
            update_accessed_date: self.update_accessed_date,
            oem_cp_converter: self.oem_cp_converter,
            time_provider,
            strict: self.strict,
        }
    }

    /// If enabled more validations are performed to check if file-system is conforming to specification.
    pub fn strict(self, strict: bool) -> Self {
        Self {
            update_accessed_date: self.update_accessed_date,
            oem_cp_converter: self.oem_cp_converter,
            time_provider: self.time_provider,
            strict,
        }
    }
}

/// A FAT volume statistics.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct FileSystemStats {
    cluster_size: u32,
    total_clusters: u32,
    free_clusters: u32,
}

impl FileSystemStats {
    /// Cluster size in bytes
    #[must_use]
    pub fn cluster_size(&self) -> u32 {
        self.cluster_size
    }

    /// Number of total clusters in filesystem usable for file allocation
    #[must_use]
    pub fn total_clusters(&self) -> u32 {
        self.total_clusters
    }

    /// Number of free clusters
    #[must_use]
    pub fn free_clusters(&self) -> u32 {
        self.free_clusters
    }
}

/// A FAT filesystem object.
///
/// `FileSystem` struct is representing a state of a mounted FAT volume.
pub struct FileSystem<IO: ReadWriteSeek, TP = DefaultTimeProvider, OCC = LossyOemCpConverter> {
    pub(crate) disk: RefCell<IO>,
    pub(crate) options: FsOptions<TP, OCC>,
    fat_type: FatType,
    bpb: BiosParameterBlock,
    first_data_sector: u32,
    root_dir_sectors: u32,
    total_clusters: u32,
    fs_info: RefCell<FsInfoSector>,
    current_status_flags: Cell<FsStatusFlags>,
    // Modified for PaperPaw: notes about directories (see `Dir::note`).
    dir_notes: RefCell<[Option<DirNote>; DIR_NOTES]>,
    entries_written: Cell<u64>,
    // Modified for PaperPaw: see `cluster_changes`.
    clusters_taken: Cell<u64>,
    clusters_freed: Cell<u64>,
    // Modified for PaperPaw: see `before_fat_write`.
    count_kept: Cell<bool>,
    // Modified for PaperPaw: the claim the FS information sector holds now (see `claim`).
    claim: Cell<Option<claim::Claim>>,
    // Modified for PaperPaw: see `check_kept_places`.
    #[cfg(debug_assertions)]
    check_places: Cell<bool>,
}

// Modified for PaperPaw: added.
/// A note about a directory (see `Dir::note`): its first cluster, the note, and a place in its
/// cluster chain (see `File::chain_at`) where `Dir::iter_from` seeks from. A directory's chain
/// only grows at its end while it is there, so a place stays right as long as the note does.
///
/// That holds only while whatever frees a directory's clusters or changes its chain otherwise than
/// by adding a cluster at its end drops its note first (`drop_dir_note`), as `Dir::remove_found`
/// does for a directory removed: a directory cut shorter, repaired, or freed by a settling that
/// frees clusters instead of unlinking an entry must too, or a directory made later on its first
/// cluster would be read at the old one's places.
#[derive(Clone, Copy)]
struct DirNote {
    cluster: u32,
    note: u64,
    chain: Option<(u32, u32)>,
}

/// How many directories are noted at most (see `Dir::note`): the ones noted last. Enough for a
/// folder of books and the large folders of what is kept for each book (their parts) to stay
/// noted while each book is taken in turn; 24 bytes each.
pub const DIR_NOTES: usize = 8;

pub trait IntoStorage<T: Read + Write + Seek> {
    fn into_storage(self) -> T;
}

impl<T: Read + Write + Seek> IntoStorage<T> for T {
    fn into_storage(self) -> Self {
        self
    }
}

#[cfg(feature = "std")]
impl<T: std::io::Read + std::io::Write + std::io::Seek> IntoStorage<io::StdIoWrapper<T>> for T {
    fn into_storage(self) -> io::StdIoWrapper<Self> {
        io::StdIoWrapper::new(self)
    }
}

impl<IO: Read + Write + Seek, TP, OCC> FileSystem<IO, TP, OCC> {
    /// Creates a new filesystem object instance.
    ///
    /// Supplied `storage` parameter can be a non-partitioned device (like FDD) or a single
    /// partition of a partitioned device (like HDD or SSD). Partitioned device must be
    /// wrapped in a wrapper that limits access to partition bytes only (e.g. `fscommon::StreamSlice`),
    /// so offet 0 is at the start of the partition.
    /// The implementation seeks to the position 0, before reading anything.
    ///
    /// It is strongly recommended to use buffered I/O with `storage`.
    /// This library frequently performs small read and write operations; passing an
    /// unbuffered storage implementation will result in severe performance degradation.
    /// Because I/O buffering strategies are often OS-dependent, buffering is left to the caller.
    ///
    /// Note: creating multiple filesystem objects with a single underlying storage can
    /// cause a filesystem corruption.
    ///
    /// # Errors
    ///
    /// Errors that can be returned:
    ///
    /// * `Error::CorruptedFileSystem` will be returned if the boot sector and/or the file system information sector
    ///   contains invalid values.
    /// * `Error::Io` will be returned if the provided storage object returned an I/O error.
    ///
    /// # Panics
    ///
    /// Panics in non-optimized build if `storage` position returned by `seek` is not zero.
    pub fn new<T: IntoStorage<IO>>(
        storage: T,
        options: FsOptions<TP, OCC>,
    ) -> Result<Self, Error<IO::Error>> {
        // seek to the volume beggining
        let mut disk = storage.into_storage();
        trace!("FileSystem::new");
        let current_pos = disk.seek(SeekFrom::Current(0))?;
        if disk.seek(SeekFrom::Current(0))? != 0 {
            debug!(
                "Seeking from position {} to the start of the volume",
                current_pos
            );
            disk.seek(SeekFrom::Start(0))?;
        }

        // read boot sector
        let bpb = {
            let boot = BootSector::deserialize(&mut disk)?;
            boot.validate(options.strict)?;
            boot.bpb
        };

        let root_dir_sectors = bpb.root_dir_sectors();
        let first_data_sector = bpb.first_data_sector();
        let total_clusters = bpb.total_clusters();
        let fat_type = FatType::from_clusters(total_clusters);
        // Modified for PaperPaw: FAT12 and FAT16 volumes only where built in (feature `fat16`).
        if !cfg!(feature = "fat16") && fat_type != FatType::Fat32 {
            return Err(Error::CorruptedFileSystem);
        }

        // read FSInfo sector if this is FAT32
        let mut fs_info = if fat_type == FatType::Fat32 {
            disk.seek(SeekFrom::Start(
                bpb.bytes_from_sectors(bpb.fs_info_sector()),
            ))?;
            FsInfoSector::deserialize(&mut disk)?
        } else {
            FsInfoSector::default()
        };

        // Modified for PaperPaw: the count in FSInfo is never taken here, dirty flag or not: it
        // may be wrong on a volume that was never marked dirty (cut off while it was written by
        // a device that marks none). `stats` counts the FAT; a caller that kept the count itself
        // reads it with `kept_free_count`.
        fs_info.free_cluster_count = None;

        // Validate the numbers stored in the free_cluster_count and next_free_cluster are within bounds for volume
        fs_info.validate_and_fix(total_clusters);

        // Modified for PaperPaw: a claim left by a cut waits for `settle_claim`.
        let claim = Self::stale_claim(fs_info.claimed);

        // return FileSystem struct
        let status_flags = bpb.status_flags();
        trace!("FileSystem::new end");
        Ok(Self {
            disk: RefCell::new(disk),
            options,
            fat_type,
            bpb,
            first_data_sector,
            root_dir_sectors,
            total_clusters,
            fs_info: RefCell::new(fs_info),
            current_status_flags: Cell::new(status_flags),
            dir_notes: RefCell::new([None; DIR_NOTES]),
            entries_written: Cell::new(0),
            clusters_taken: Cell::new(0),
            clusters_freed: Cell::new(0),
            // Whatever count the sector holds as mounted: someone may trust it.
            count_kept: Cell::new(fat_type == FatType::Fat32),
            claim: Cell::new(claim),
            #[cfg(debug_assertions)]
            check_places: Cell::new(false),
        })
    }

    /// Returns a type of File Allocation Table (FAT) used by this filesystem.
    pub fn fat_type(&self) -> FatType {
        self.fat_type
    }

    /// Returns a volume identifier read from BPB in the Boot Sector.
    pub fn volume_id(&self) -> u32 {
        self.bpb.volume_id
    }

    /// Returns a volume label from BPB in the Boot Sector as byte array slice.
    ///
    /// Label is encoded in the OEM codepage.
    /// Note: This function returns label stored in the BPB block. Use `read_volume_label_from_root_dir_as_bytes` to
    /// read label from the root directory.
    pub fn volume_label_as_bytes(&self) -> &[u8] {
        let full_label_slice = &self.bpb.volume_label;
        let len = full_label_slice
            .iter()
            .rposition(|b| *b != SFN_PADDING)
            .map_or(0, |p| p + 1);
        &full_label_slice[..len]
    }

    fn offset_from_sector(&self, sector: u32) -> u64 {
        self.bpb.bytes_from_sectors(sector)
    }

    fn sector_from_cluster(&self, cluster: u32) -> u32 {
        self.first_data_sector
            + self
                .bpb
                .sectors_from_clusters(cluster - RESERVED_FAT_ENTRIES)
    }

    pub fn cluster_size(&self) -> u32 {
        self.bpb.cluster_size()
    }

    // Modified for PaperPaw: added.

    /// Clusters of the volume's data: they are numbered from 2 to this many and one.
    #[must_use]
    pub fn total_clusters(&self) -> u32 {
        self.total_clusters
    }

    // Modified for PaperPaw: `dir_note`, `set_dir_note`, `dir_chain`, `set_dir_chain`,
    // `entries_written`, `note_entry_written`, `drop_dir_note` and `forget_dir_notes` added (see
    // `Dir::note`).

    pub(crate) fn dir_note(&self, cluster: u32) -> Option<u64> {
        let notes = self.dir_notes.borrow();
        notes
            .iter()
            .flatten()
            .find(|n| n.cluster == cluster)
            .map(|n| n.note)
    }

    pub(crate) fn set_dir_note(&self, cluster: u32, note: u64) {
        let mut notes = self.dir_notes.borrow_mut();
        // The one noted last first: the oldest goes. Its place in the chain stays.
        let at = notes
            .iter()
            .position(|n| n.is_some_and(|n| n.cluster == cluster));
        let chain = at.and_then(|at| notes[at]).and_then(|n| n.chain);
        let at = at.unwrap_or(DIR_NOTES - 1);
        notes[..=at].rotate_right(1);
        notes[0] = Some(DirNote {
            cluster,
            note,
            chain,
        });
    }

    /// The place in the chain of the directory beginning at `cluster` kept with its note.
    pub(crate) fn dir_chain(&self, cluster: u32) -> Option<(u32, u32)> {
        let notes = self.dir_notes.borrow();
        notes
            .iter()
            .flatten()
            .find(|n| n.cluster == cluster)
            .and_then(|n| n.chain)
    }

    /// Keeps `chain` as the place in the chain of the directory beginning at `cluster`, where it
    /// is noted.
    pub(crate) fn set_dir_chain(&self, cluster: u32, chain: (u32, u32)) {
        let mut notes = self.dir_notes.borrow_mut();
        if let Some(n) = notes.iter_mut().flatten().find(|n| n.cluster == cluster) {
            n.chain = Some(chain);
        }
    }

    /// How many entries were written so far (files and directories created, entries moved or
    /// renamed), in any directory.
    #[must_use]
    pub fn entries_written(&self) -> u64 {
        self.entries_written.get()
    }

    pub(crate) fn note_entry_written(&self, cluster: Option<u32>) {
        self.entries_written
            .set(self.entries_written.get().wrapping_add(1));
        if let Some(cluster) = cluster {
            self.drop_dir_note(cluster);
        }
    }

    /// In debug builds, for tests: whether every place kept in a directory's chain (see
    /// `DirNote`) is checked against the chain where it is used, reading the FAT along it (as
    /// many blocks as the place saves).
    #[cfg(debug_assertions)]
    pub fn check_kept_places(&self, on: bool) {
        self.check_places.set(on);
    }

    #[cfg(debug_assertions)]
    pub(crate) fn checks_kept_places(&self) -> bool {
        self.check_places.get()
    }

    /// Drops the note about the directory beginning at `cluster` (see `DirNote`): before its
    /// clusters are freed or its chain changes otherwise than at its end.
    pub(crate) fn drop_dir_note(&self, cluster: u32) {
        let mut notes = self.dir_notes.borrow_mut();
        for note in notes.iter_mut() {
            if note.is_some_and(|n| n.cluster == cluster) {
                *note = None;
            }
        }
    }

    /// Drops every note about a directory (see `Dir::note`): e.g. where the card was changed
    /// otherwise than through this file system. PaperPaw does not call it: a card is mounted
    /// anew (a new `FileSystem`) whenever it may have been changed elsewhere. A shell that lets
    /// the card be written otherwise while it stays mounted (as a USB drive, say) must call it.
    pub fn forget_dir_notes(&self) {
        *self.dir_notes.borrow_mut() = [None; DIR_NOTES];
    }

    pub(crate) fn offset_from_cluster(&self, cluster: u32) -> u64 {
        self.offset_from_sector(self.sector_from_cluster(cluster))
    }

    pub(crate) fn bytes_from_clusters(&self, clusters: u32) -> u64 {
        self.bpb
            .bytes_from_sectors(self.bpb.sectors_from_clusters(clusters))
    }

    pub(crate) fn clusters_from_bytes(&self, bytes: u64) -> u32 {
        self.bpb.clusters_from_bytes(bytes)
    }

    fn fat_slice(&self) -> impl ReadWriteSeek<Error = Error<IO::Error>> + '_ {
        let io = FsIoAdapter { fs: self };
        fat_slice(io, &self.bpb)
    }

    pub(crate) fn cluster_iter(
        &self,
        cluster: u32,
    ) -> ClusterIterator<impl ReadWriteSeek<Error = Error<IO::Error>> + '_, IO::Error> {
        let disk_slice = self.fat_slice();
        ClusterIterator::new(disk_slice, self.fat_type, cluster)
    }

    // Modified for PaperPaw: the clusters freed counted (see `cluster_changes`), also of a chain
    // freed in part.
    //
    // Modified for PaperPaw: on FAT16 and FAT32 the chain after `cluster` is freed a block of the
    // FAT at a time, each written once, and said on the disk first where a cut could leave a
    // part of it taken (see `claim`).
    pub(crate) fn truncate_cluster_chain(&self, cluster: u32) -> Result<(), Error<IO::Error>> {
        if self.fat_type != FatType::Fat12 {
            match read_fat(&mut self.fat_slice(), self.fat_type, cluster)? {
                // A link out of the volume (a damaged chain) is cut as it was.
                FatValue::Data(first) if self.is_cluster(first) => {
                    return self.truncate_claimed(cluster, first);
                }
                _ => {}
            }
        }
        let mut iter = self.cluster_iter(cluster);
        let result = iter.truncate();
        self.note_freed(iter.freed());
        self.clusters_taken.set(
            self.clusters_taken
                .get()
                .wrapping_add(u64::from(iter.taken())),
        );
        let num_free = result?;
        let mut fs_info = self.fs_info.borrow_mut();
        fs_info.map_free_clusters(|n| n.saturating_sub(iter.taken()).saturating_add(num_free));
        Ok(())
    }

    // Modified for PaperPaw: as `truncate_cluster_chain`: through `free_claimed`, which on FAT12
    // comes here (`free_chain_one_by_one`).
    pub(crate) fn free_cluster_chain(&self, cluster: u32) -> Result<(), Error<IO::Error>> {
        self.free_claimed(cluster, false)
    }

    fn free_chain_one_by_one(&self, cluster: u32) -> Result<(), Error<IO::Error>> {
        let mut iter = self.cluster_iter(cluster);
        let result = iter.free();
        self.note_freed(iter.freed());
        let num_free = result?;
        let mut fs_info = self.fs_info.borrow_mut();
        fs_info.map_free_clusters(|n| n.saturating_add(num_free));
        Ok(())
    }

    // Modified for PaperPaw: added.
    fn note_freed(&self, freed: u32) {
        self.clusters_freed
            .set(self.clusters_freed.get().wrapping_add(u64::from(freed)));
    }

    // Modified for PaperPaw: added.
    /// How many clusters this file system marked taken and how many it marked free in the FAT
    /// since it was mounted, counted as each mark is written: by files and directories written,
    /// grown, truncated and removed, also by one that failed part of the way. Counting the FAT's
    /// free clusters once and following these keeps the count exact without reading the FAT
    /// again; the FS information sector's count is not this (it may be wrong after a cut).
    #[must_use]
    pub fn cluster_changes(&self) -> (u64, u64) {
        (self.clusters_taken.get(), self.clusters_freed.get())
    }

    // Modified for PaperPaw: added.
    /// Before the FAT is written, the first time since the volume was mounted or a count was
    /// kept (`keep_free_count`): the count of free clusters in the FS information sector (FAT32)
    /// is taken back on the disk, written as unknown with the sector's reserved bytes zero. The
    /// disk takes changes in the order they were made, so after a cut nothing reads a count there
    /// that the FAT no longer agrees with; upstream left the count as it was mounted until the
    /// volume was unmounted, which a device that is switched off never does. The disk's position
    /// is left where it was.
    fn before_fat_write(&self) -> Result<(), IO::Error> {
        if !self.count_kept.get() {
            return Ok(());
        }
        let mut fs_info = self.fs_info.borrow_mut();
        fs_info.free_cluster_count = None;
        let mut disk = self.disk.borrow_mut();
        let was = disk.seek(SeekFrom::Current(0))?;
        disk.seek(SeekFrom::Start(
            self.offset_from_sector(u32::from(self.bpb.fs_info_sector)),
        ))?;
        fs_info.serialize(&mut *disk).map_err(|e| match e {
            Error::Io(e) => e,
            _ => IO::Error::new_write_zero_error(),
        })?;
        fs_info.dirty = false;
        disk.seek(SeekFrom::Start(was))?;
        self.count_kept.set(false);
        self.claim.set(None);
        Ok(())
    }

    // Modified for PaperPaw: added.
    /// What the FS information sector holds on the disk now: its count of free clusters and its
    /// hint as they are written there (`0xFFFF_FFFF` for unknown), and the first of its reserved
    /// bytes into `note` (see `keep_free_count`). `None` where the volume has none (not FAT32)
    /// or it is not one.
    ///
    /// # Errors
    ///
    /// `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn kept_free_count(&self, note: &mut [u8]) -> Result<Option<(u32, u32)>, Error<IO::Error>> {
        if self.fat_type != FatType::Fat32 {
            return Ok(None);
        }
        let mut disk = self.disk.borrow_mut();
        let was = disk.seek(SeekFrom::Current(0))?;
        disk.seek(SeekFrom::Start(
            self.offset_from_sector(u32::from(self.bpb.fs_info_sector)),
        ))?;
        let lead = disk.read_u32_le()?;
        let mut reserved = [0_u8; 480];
        disk.read_exact(&mut reserved)?;
        let struc = disk.read_u32_le()?;
        let free = disk.read_u32_le()?;
        let next = disk.read_u32_le()?;
        disk.seek(SeekFrom::Start(was))?;
        if lead != FsInfoSector::LEAD_SIG || struc != FsInfoSector::STRUC_SIG {
            return Ok(None);
        }
        let noted = note.len().min(reserved.len());
        note[..noted].copy_from_slice(&reserved[..noted]);
        Ok(Some((free, next)))
    }

    // Modified for PaperPaw: added.
    /// Writes the FS information sector (FAT32) with `free` as its count of free clusters, the
    /// hint as it is followed, and `note` in the first of its reserved bytes: a caller's own
    /// word that it counted them, which no other writer of the sector leaves there with another
    /// count. For a caller that has written everything back and counted the FAT exactly. The
    /// volume's dirty mark is left as it was mounted: PaperPaw sets none, so one found is another
    /// writer's (a computer that was not let go of the card), and it is theirs to check. The count
    /// is taken back before the FAT is written next (`before_fat_write`). `false` where the volume
    /// has no such sector.
    ///
    /// # Errors
    ///
    /// `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn keep_free_count(&self, free: u32, note: &[u8]) -> Result<bool, Error<IO::Error>> {
        if self.fat_type != FatType::Fat32 {
            return Ok(false);
        }
        {
            let mut fs_info = self.fs_info.borrow_mut();
            fs_info.free_cluster_count = Some(free);
            let mut disk = self.disk.borrow_mut();
            let was = disk.seek(SeekFrom::Current(0))?;
            disk.seek(SeekFrom::Start(
                self.offset_from_sector(u32::from(self.bpb.fs_info_sector)),
            ))?;
            fs_info.serialize_noted(&mut *disk, note)?;
            fs_info.dirty = false;
            disk.seek(SeekFrom::Start(was))?;
        }
        self.count_kept.set(true);
        // Overwritten with the count's record.
        self.claim.set(None);
        Ok(true)
    }

    // Modified for PaperPaw: added.
    /// Where the next free cluster is looked for first, as `keep_free_count` writes it to the FS
    /// information sector: `0xFFFF_FFFF` where it is not known.
    #[must_use]
    pub fn next_free_hint(&self) -> u32 {
        self.fs_info
            .borrow()
            .next_free_cluster
            .unwrap_or(0xFFFF_FFFF)
    }

    // Modified for PaperPaw: added.
    /// Whether a count of free clusters may be in the FS information sector on the disk: as it
    /// was mounted or as kept (`keep_free_count`), the FAT not written since.
    #[must_use]
    pub fn free_count_kept(&self) -> bool {
        self.count_kept.get()
    }

    // Modified for PaperPaw: added.
    /// Whether the volume is marked dirty in its boot sector: as it was mounted, or as marked
    /// since.
    #[must_use]
    pub fn marked_dirty(&self) -> bool {
        self.current_status_flags.get().dirty
    }

    // Modified for PaperPaw: `referrer` added, what is to point at the cluster: said on the disk
    // before the cluster is marked taken (see `claim`).
    //
    // Modified for PaperPaw: a directory's cluster, zeroed before anything links it (`zero` is
    // gone: every caller zeroed). Upstream linked it from `prev_cluster` first: the disk takes
    // changes in the order they were made, so a cut before the zeros left the bytes the cluster
    // held before (a removed file's) in the directory, read as entries whose clusters are other
    // files' (removing the directory then freed them). Now the cluster is marked taken on its
    // own, zeroed, and linked last; cut off before the link, it is taken by nothing, and the
    // claim of its link (`Referrer::Link`) has the next mount free it.
    pub(crate) fn alloc_cluster(
        &self,
        prev_cluster: Option<u32>,
        referrer: Referrer,
    ) -> Result<u32, Error<IO::Error>> {
        trace!("alloc_cluster");
        let hint = self.fs_info.borrow().next_free_cluster;
        let (cluster, _) = {
            let mut fat = self.fat_slice();
            crate::table::alloc_run(
                &mut fat,
                self.fat_type,
                None,
                hint,
                self.total_clusters,
                &self.clusters_taken,
                1,
                &mut |fat, first, entries, _| self.claim_taking(fat, first, entries, referrer),
            )?
        };
        {
            let mut disk = self.disk.borrow_mut();
            disk.seek(SeekFrom::Start(self.offset_from_cluster(cluster)))?;
            write_zeros(&mut *disk, u64::from(self.cluster_size()))?;
        }
        if let Some(prev) = prev_cluster {
            write_fat(
                &mut self.fat_slice(),
                self.fat_type,
                prev,
                FatValue::Data(cluster),
            )?;
        }
        let mut fs_info = self.fs_info.borrow_mut();
        fs_info.set_next_free_cluster(cluster + 1);
        // Modified for PaperPaw: a count kept by someone else may be too low.
        fs_info.map_free_clusters(|n| n.saturating_sub(1));
        Ok(cluster)
    }

    // Modified for PaperPaw: added. A run of up to `want` free clusters in a row taken at once, linked
    // from `prev_cluster` and to one another (see `table::alloc_run`); the number of the first and how
    // many came. Not for a directory (those clusters are zeroed one at a time). `entry`: what is
    // to point at the first cluster of a file that has none. Where the clusters and what points
    // at them are not one write, they are said on the disk first (see `claim`).
    pub(crate) fn alloc_clusters(
        &self,
        prev_cluster: Option<u32>,
        want: u32,
        entry: Referrer,
    ) -> Result<(u32, u32), Error<IO::Error>> {
        trace!("alloc_clusters");
        let referrer = prev_cluster.map_or(entry, Referrer::Link);
        let hint = self.fs_info.borrow().next_free_cluster;
        let (first, count) = {
            let mut fat = self.fat_slice();
            crate::table::alloc_run(
                &mut fat,
                self.fat_type,
                prev_cluster,
                hint,
                self.total_clusters,
                &self.clusters_taken,
                want,
                &mut |fat, first, entries, joined| {
                    if joined {
                        self.drop_free_claim()
                    } else {
                        self.claim_taking(fat, first, entries, referrer)
                    }
                },
            )?
        };
        let mut fs_info = self.fs_info.borrow_mut();
        fs_info.set_next_free_cluster(first + count);
        fs_info.map_free_clusters(|n| n.saturating_sub(count));
        Ok((first, count))
    }

    // Modified for PaperPaw: added. The clusters after `cluster` freed, `cluster` ending its chain: as
    // `truncate_cluster_chain`, but nothing is written where nothing comes after it.
    pub(crate) fn truncate_cluster_chain_run(&self, cluster: u32) -> Result<(), Error<IO::Error>> {
        if self.fat_type != FatType::Fat12 {
            match read_fat(&mut self.fat_slice(), self.fat_type, cluster)? {
                FatValue::Data(first) if self.is_cluster(first) => {
                    return self.truncate_claimed(cluster, first);
                }
                FatValue::EndOfChain => return Ok(()),
                _ => {}
            }
        }
        self.truncate_cluster_chain(cluster)
    }

    /// Returns status flags for this volume.
    ///
    /// # Errors
    ///
    /// `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn read_status_flags(&self) -> Result<FsStatusFlags, Error<IO::Error>> {
        let bpb_status = self.bpb.status_flags();
        let fat_status = read_fat_flags(&mut self.fat_slice(), self.fat_type)?;
        Ok(FsStatusFlags {
            dirty: bpb_status.dirty || fat_status.dirty,
            io_error: bpb_status.io_error || fat_status.io_error,
        })
    }

    /// Returns filesystem statistics like number of total and free clusters.
    ///
    /// For FAT32 volumes number of free clusters from the FS Information Sector is returned (may be incorrect).
    /// For other FAT variants number is computed on the first call to this method and cached for later use.
    ///
    /// # Errors
    ///
    /// `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn stats(&self) -> Result<FileSystemStats, Error<IO::Error>> {
        let free_clusters_option = self.fs_info.borrow().free_cluster_count;
        let free_clusters = if let Some(n) = free_clusters_option {
            n
        } else {
            self.recalc_free_clusters()?
        };
        Ok(FileSystemStats {
            cluster_size: self.cluster_size(),
            total_clusters: self.total_clusters,
            free_clusters,
        })
    }

    /// Forces free clusters recalculation.
    fn recalc_free_clusters(&self) -> Result<u32, Error<IO::Error>> {
        let mut fat = self.fat_slice();
        let free_cluster_count = count_free_clusters(&mut fat, self.fat_type, self.total_clusters)?;
        self.fs_info
            .borrow_mut()
            .set_free_cluster_count(free_cluster_count);
        Ok(free_cluster_count)
    }

    /// Flush filesystem bookkeeping and the underlying storage without unmounting.
    ///
    /// Open file handles must be flushed first to publish their directory entries.
    /// The mounted dirty flag is retained; only `unmount` marks a clean release.
    /// This does not invalidate a cache supplied by the caller's storage adapter.
    ///
    /// # Errors
    ///
    /// Returns `Error::Io` when bookkeeping or the storage flush fails.
    pub fn flush(&self) -> Result<(), Error<IO::Error>> {
        self.flush_fs_info()?;
        self.disk.borrow_mut().flush().map_err(Error::Io)
    }

    /// Unmounts the filesystem.
    ///
    /// Updates the FS Information Sector if needed.
    ///
    /// # Errors
    ///
    /// `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn unmount(self) -> Result<(), Error<IO::Error>> {
        self.unmount_internal()
    }

    fn unmount_internal(&self) -> Result<(), Error<IO::Error>> {
        self.flush_fs_info()?;
        self.set_dirty_flag(false)?;
        Ok(())
    }

    fn flush_fs_info(&self) -> Result<(), Error<IO::Error>> {
        let mut fs_info = self.fs_info.borrow_mut();
        if self.fat_type == FatType::Fat32 && fs_info.dirty {
            let mut disk = self.disk.borrow_mut();
            let fs_info_sector_offset = self.offset_from_sector(u32::from(self.bpb.fs_info_sector));
            disk.seek(SeekFrom::Start(fs_info_sector_offset))?;
            fs_info.serialize(&mut *disk)?;
            fs_info.dirty = false;
            // Modified for PaperPaw: see `before_fat_write`.
            self.count_kept.set(true);
            self.claim.set(None);
        }
        Ok(())
    }

    pub(crate) fn set_dirty_flag(&self, dirty: bool) -> Result<(), IO::Error> {
        // Do not overwrite flags read from BPB on mount
        let mut flags = self.bpb.status_flags();
        flags.dirty |= dirty;
        // Check if flags has changed
        let current_flags = self.current_status_flags.get();
        if flags == current_flags {
            // Nothing to do
            return Ok(());
        }
        let encoded = flags.encode();
        // Note: only one field is written to avoid rewriting entire boot-sector which could be dangerous
        // Compute reserver_1 field offset and write new flags
        let offset = if self.fat_type() == FatType::Fat32 {
            0x041
        } else {
            0x025
        };
        let mut disk = self.disk.borrow_mut();
        disk.seek(io::SeekFrom::Start(offset))?;
        disk.write_u8(encoded)?;
        self.current_status_flags.set(flags);
        Ok(())
    }

    /// Returns a root directory object allowing for futher penetration of a filesystem structure.
    pub fn root_dir(&self) -> Dir<'_, IO, TP, OCC> {
        trace!("root_dir");
        let root_rdr = {
            match self.fat_type {
                #[cfg(feature = "fat16")]
                FatType::Fat12 | FatType::Fat16 => DirRawStream::Root(DiskSlice::from_sectors(
                    self.first_data_sector - self.root_dir_sectors,
                    self.root_dir_sectors,
                    1,
                    &self.bpb,
                    FsIoAdapter { fs: self },
                )),
                _ => {
                    DirRawStream::File(File::new(Some(self.bpb.root_dir_first_cluster), None, self))
                }
            }
        };
        Dir::new(root_rdr, self)
    }
}

impl<IO: ReadWriteSeek, TP, OCC: OemCpConverter> FileSystem<IO, TP, OCC> {
    /// Returns a volume label from BPB in the Boot Sector as `String`.
    ///
    /// Non-ASCII characters are replaced by the replacement character (U+FFFD).
    /// Note: This function returns label stored in the BPB block. Use `read_volume_label_from_root_dir` to read label
    /// from the root directory.
    pub fn volume_label(&self) -> String {
        // Decode volume label from OEM codepage
        let volume_label_iter = self.volume_label_as_bytes().iter().copied();
        let char_iter = volume_label_iter.map(|c| self.options.oem_cp_converter.decode(c));
        // Build string from character iterator
        char_iter.collect()
    }
}

impl<IO: ReadWriteSeek, TP: TimeProvider, OCC: OemCpConverter> FileSystem<IO, TP, OCC> {
    /// Returns a volume label from root directory as `String`.
    ///
    /// It finds file with `VOLUME_ID` attribute and returns its short name.
    ///
    /// # Errors
    ///
    /// `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn read_volume_label_from_root_dir(&self) -> Result<Option<String>, Error<IO::Error>> {
        // Note: DirEntry::file_short_name() cannot be used because it interprets name as 8.3
        // (adds dot before an extension)
        let volume_label_opt = self.read_volume_label_from_root_dir_as_bytes()?;
        volume_label_opt.map_or(Ok(None), |volume_label| {
            // Strip label padding
            let len = volume_label
                .iter()
                .rposition(|b| *b != SFN_PADDING)
                .map_or(0, |p| p + 1);
            let label_slice = &volume_label[..len];
            // Decode volume label from OEM codepage
            let volume_label_iter = label_slice.iter().copied();
            let char_iter = volume_label_iter.map(|c| self.options.oem_cp_converter.decode(c));
            // Build string from character iterator
            Ok(Some(char_iter.collect::<String>()))
        })
    }

    /// Returns a volume label from root directory as byte array.
    ///
    /// Label is encoded in the OEM codepage.
    /// It finds file with `VOLUME_ID` attribute and returns its short name.
    ///
    /// # Errors
    ///
    /// `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn read_volume_label_from_root_dir_as_bytes(
        &self,
    ) -> Result<Option<[u8; SFN_SIZE]>, Error<IO::Error>> {
        let entry_opt = self.root_dir().find_volume_entry()?;
        Ok(entry_opt.map(|e| *e.raw_short_name()))
    }
}

/// `Drop` implementation tries to unmount the filesystem when dropping.
impl<IO: ReadWriteSeek, TP, OCC> Drop for FileSystem<IO, TP, OCC> {
    fn drop(&mut self) {
        if let Err(err) = self.unmount_internal() {
            error!("unmount failed {:?}", err);
        }
    }
}

pub(crate) struct FsIoAdapter<'a, IO: ReadWriteSeek, TP, OCC> {
    fs: &'a FileSystem<IO, TP, OCC>,
}

impl<IO: ReadWriteSeek, TP, OCC> IoBase for FsIoAdapter<'_, IO, TP, OCC> {
    type Error = IO::Error;
}

impl<IO: ReadWriteSeek, TP, OCC> Read for FsIoAdapter<'_, IO, TP, OCC> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.fs.disk.borrow_mut().read(buf)
    }
}

impl<IO: ReadWriteSeek, TP, OCC> Write for FsIoAdapter<'_, IO, TP, OCC> {
    // Modified for PaperPaw: the FAT (and the root directory of FAT12 and FAT16) is written
    // through here. Upstream marked the volume dirty in its boot sector after the first write and
    // never cleared the mark of a volume that is not unmounted, so every card PaperPaw wrote was
    // dirty for good and no reader of it kept the FS information sector. Now the count in that
    // sector is taken back before the write instead (`before_fat_write`), and the boot sector's
    // mark is left alone.
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        if !buf.is_empty() {
            self.fs.before_fat_write()?;
        }
        self.fs.disk.borrow_mut().write(buf)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.fs.disk.borrow_mut().flush()
    }
}

impl<IO: ReadWriteSeek, TP, OCC> Seek for FsIoAdapter<'_, IO, TP, OCC> {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, Self::Error> {
        self.fs.disk.borrow_mut().seek(pos)
    }
}

// Note: derive cannot be used because of invalid bounds. See: https://github.com/rust-lang/rust/issues/26925
impl<IO: ReadWriteSeek, TP, OCC> Clone for FsIoAdapter<'_, IO, TP, OCC> {
    fn clone(&self) -> Self {
        FsIoAdapter { fs: self.fs }
    }
}

pub(crate) fn fat_slice<S: ReadWriteSeek, B: BorrowMut<S>>(
    io: B,
    bpb: &BiosParameterBlock,
) -> impl ReadWriteSeek<Error = Error<S::Error>> {
    let sectors_per_fat = bpb.sectors_per_fat();
    let mirroring_enabled = bpb.mirroring_enabled();
    let (fat_first_sector, mirrors) = if mirroring_enabled {
        (bpb.reserved_sectors(), bpb.fats)
    } else {
        let active_fat = u32::from(bpb.active_fat());
        let fat_first_sector = (bpb.reserved_sectors()) + active_fat * sectors_per_fat;
        (fat_first_sector, 1)
    };
    DiskSlice::from_sectors(fat_first_sector, sectors_per_fat, mirrors, bpb, io)
}

pub(crate) struct DiskSlice<B, S = B> {
    begin: u64,
    size: u64,
    offset: u64,
    mirrors: u8,
    inner: B,
    phantom: PhantomData<S>,
}

impl<B: BorrowMut<S>, S: ReadWriteSeek> DiskSlice<B, S> {
    pub(crate) fn new(begin: u64, size: u64, mirrors: u8, inner: B) -> Self {
        Self {
            begin,
            size,
            mirrors,
            inner,
            offset: 0,
            phantom: PhantomData,
        }
    }

    fn from_sectors(
        first_sector: u32,
        sector_count: u32,
        mirrors: u8,
        bpb: &BiosParameterBlock,
        inner: B,
    ) -> Self {
        Self::new(
            bpb.bytes_from_sectors(first_sector),
            bpb.bytes_from_sectors(sector_count),
            mirrors,
            inner,
        )
    }

    pub(crate) fn abs_pos(&self) -> u64 {
        self.begin + self.offset
    }
}

// Note: derive cannot be used because of invalid bounds. See: https://github.com/rust-lang/rust/issues/26925
impl<B: Clone, S> Clone for DiskSlice<B, S> {
    fn clone(&self) -> Self {
        Self {
            begin: self.begin,
            size: self.size,
            offset: self.offset,
            mirrors: self.mirrors,
            inner: self.inner.clone(),
            // phantom is needed to add type bounds on the storage type
            phantom: PhantomData,
        }
    }
}

impl<B, S: IoBase> IoBase for DiskSlice<B, S> {
    type Error = Error<S::Error>;
}

impl<B: BorrowMut<S>, S: Read + Seek> Read for DiskSlice<B, S> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let offset = self.begin + self.offset;
        let read_size = (buf.len() as u64).min(self.size - self.offset) as usize;
        self.inner.borrow_mut().seek(SeekFrom::Start(offset))?;
        let size = self.inner.borrow_mut().read(&mut buf[..read_size])?;
        self.offset += size as u64;
        Ok(size)
    }
}

impl<B: BorrowMut<S>, S: Write + Seek> Write for DiskSlice<B, S> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        let offset = self.begin + self.offset;
        let write_size = (buf.len() as u64).min(self.size - self.offset) as usize;
        if write_size == 0 {
            return Ok(0);
        }
        // Write data
        let storage = self.inner.borrow_mut();
        for i in 0..self.mirrors {
            storage.seek(SeekFrom::Start(offset + u64::from(i) * self.size))?;
            storage.write_all(&buf[..write_size])?;
        }
        self.offset += write_size as u64;
        Ok(write_size)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(self.inner.borrow_mut().flush()?)
    }
}

impl<B, S: IoBase> Seek for DiskSlice<B, S> {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, Self::Error> {
        let new_offset_opt: Option<u64> = match pos {
            SeekFrom::Current(x) => i64::try_from(self.offset)
                .ok()
                .and_then(|n| n.checked_add(x))
                .and_then(|n| u64::try_from(n).ok()),
            SeekFrom::Start(x) => Some(x),
            SeekFrom::End(o) => i64::try_from(self.size)
                .ok()
                .and_then(|size| size.checked_add(o))
                .and_then(|n| u64::try_from(n).ok()),
        };
        if let Some(new_offset) = new_offset_opt {
            if new_offset > self.size {
                error!("Seek beyond the end of the file");
                Err(Error::InvalidInput)
            } else {
                self.offset = new_offset;
                Ok(self.offset)
            }
        } else {
            error!("Invalid seek offset");
            Err(Error::InvalidInput)
        }
    }
}

/// An OEM code page encoder/decoder.
///
/// Provides a custom implementation for a short name encoding/decoding.
/// `OemCpConverter` is specified by the `oem_cp_converter` property in `FsOptions` struct.
pub trait OemCpConverter: Debug {
    fn decode(&self, oem_char: u8) -> char;
    fn encode(&self, uni_char: char) -> Option<u8>;
}

/// Default implementation of `OemCpConverter` that changes all non-ASCII characters to the replacement character (U+FFFD).
#[derive(Debug, Clone, Copy, Default)]
pub struct LossyOemCpConverter {
    _dummy: (),
}

impl LossyOemCpConverter {
    #[must_use]
    pub fn new() -> Self {
        Self { _dummy: () }
    }
}

impl OemCpConverter for LossyOemCpConverter {
    fn decode(&self, oem_char: u8) -> char {
        if oem_char <= 0x7F {
            char::from(oem_char)
        } else {
            '\u{FFFD}'
        }
    }
    fn encode(&self, uni_char: char) -> Option<u8> {
        if uni_char <= '\x7F' {
            Some(uni_char as u8) // safe cast: value is in range [0, 0x7F]
        } else {
            None
        }
    }
}

pub(crate) fn write_zeros<IO: ReadWriteSeek>(disk: &mut IO, len: u64) -> Result<(), IO::Error> {
    disk.write_zeros(len)
}

// Modified for PaperPaw: clusters a cut would leave taken by no file.
#[path = "claim.rs"]
mod claim;
pub(crate) use claim::Referrer;

// Modified for PaperPaw: corrupt-chain accounting regressions.
#[cfg(test)]
#[path = "fs_count_tests.rs"]
mod count_tests;
