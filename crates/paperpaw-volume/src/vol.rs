//! One interface over the crate's volume types: [`Volume::new`] reads the
//! boot sector ([`crate::detect`]) and mounts FAT ([`crate::FileSystem`]) or
//! exFAT (`crate::exfat`); [`Dir`], [`File`] and [`DirEntry`] are the handles
//! of either, with the methods of the FAT ones. A caller written for FAT
//! (PaperPaw's storage, `paperpaw-fat`) runs on exFAT unchanged.
//!
//! What exFAT has no use for answers as a FAT volume without it would: no
//! short names, no clusters taken ahead of an append, no claim in an FS
//! information sector, no count of free clusters kept on the card.

use alloc::string::String;

use crate::dir_entry::FileAttributes;
use crate::error::Error;
use crate::exfat::{ExFat, XDir, XEntry, XFile, XIter};
use crate::fs::{FileSystem, FsOptions, LossyOemCpConverter, ReadWriteSeek};
use crate::io::{IoBase, Read, Seek, SeekFrom, Write};
use crate::kind::{VolumeKind, detect};
use crate::time::{DateTime, DefaultTimeProvider};

type Tp = DefaultTimeProvider;
type Occ = LossyOemCpConverter;
type Res<T, IO> = Result<T, Error<<IO as IoBase>::Error>>;

/// A mounted volume, FAT or exFAT.
pub enum Volume<IO: ReadWriteSeek> {
    Fat(FileSystem<IO, Tp, Occ>),
    ExFat(ExFat<IO>),
}

impl<IO: ReadWriteSeek> core::fmt::Debug for Volume<IO> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Volume").field(&self.kind()).finish()
    }
}

impl<IO: ReadWriteSeek> From<FileSystem<IO, Tp, Occ>> for Volume<IO> {
    fn from(fs: FileSystem<IO, Tp, Occ>) -> Self {
        Self::Fat(fs)
    }
}

impl<IO: ReadWriteSeek> Volume<IO> {
    /// Mounts the volume on `disk`, whichever type its boot sector says.
    ///
    /// # Errors
    ///
    /// `Error::Io` where the disk cannot be read, `Error::CorruptedFileSystem`
    /// where its boot sector is neither type's or says what cannot be.
    pub fn new(mut disk: IO) -> Res<Self, IO> {
        let mut boot = [0u8; 512];
        disk.seek(SeekFrom::Start(0))?;
        disk.read_exact(&mut boot)?;
        disk.seek(SeekFrom::Start(0))?;
        match detect(&boot) {
            Some(VolumeKind::ExFat) => Ok(Self::ExFat(ExFat::mount(disk)?)),
            _ => Ok(Self::Fat(FileSystem::new(disk, FsOptions::new())?)),
        }
    }

    #[must_use]
    pub fn kind(&self) -> VolumeKind {
        self.borrowed().kind()
    }

    /// A handle of the volume, to copy around.
    #[must_use]
    pub fn borrowed(&self) -> VolumeRef<'_, IO> {
        match self {
            Self::Fat(fs) => VolumeRef::Fat(fs),
            Self::ExFat(fs) => VolumeRef::ExFat(fs),
        }
    }
}

/// A borrowed [`Volume`] (or a FAT file system mounted by itself).
pub enum VolumeRef<'a, IO: ReadWriteSeek> {
    Fat(&'a FileSystem<IO, Tp, Occ>),
    ExFat(&'a ExFat<IO>),
}

impl<IO: ReadWriteSeek> Clone for VolumeRef<'_, IO> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<IO: ReadWriteSeek> Copy for VolumeRef<'_, IO> {}

impl<'a, IO: ReadWriteSeek> From<&'a FileSystem<IO, Tp, Occ>> for VolumeRef<'a, IO> {
    fn from(fs: &'a FileSystem<IO, Tp, Occ>) -> Self {
        Self::Fat(fs)
    }
}

impl<'a, IO: ReadWriteSeek> From<&'a mut FileSystem<IO, Tp, Occ>> for VolumeRef<'a, IO> {
    fn from(fs: &'a mut FileSystem<IO, Tp, Occ>) -> Self {
        Self::Fat(fs)
    }
}

impl<'a, IO: ReadWriteSeek> From<&'a Volume<IO>> for VolumeRef<'a, IO> {
    fn from(volume: &'a Volume<IO>) -> Self {
        volume.borrowed()
    }
}

/// Where an exFAT volume keeps which clusters are taken: a bit for each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FreeMap {
    /// The bitmap's first byte on the volume.
    pub offset: u64,
    /// The volume's clusters, a bit each (1: taken).
    pub clusters: u32,
    /// Bytes of a cluster.
    pub cluster_size: u32,
}

/// A volume's clusters, as [`VolumeRef::stats`] counts them.
#[derive(Clone, Copy, Debug)]
pub struct Stats {
    cluster_size: u32,
    total_clusters: u32,
    free_clusters: u32,
}

impl Stats {
    #[must_use]
    pub fn cluster_size(&self) -> u32 {
        self.cluster_size
    }

    #[must_use]
    pub fn total_clusters(&self) -> u32 {
        self.total_clusters
    }

    #[must_use]
    pub fn free_clusters(&self) -> u32 {
        self.free_clusters
    }
}

impl<'a, IO: ReadWriteSeek> VolumeRef<'a, IO> {
    #[must_use]
    pub fn kind(&self) -> VolumeKind {
        match self {
            Self::Fat(_) => VolumeKind::Fat,
            Self::ExFat(_) => VolumeKind::ExFat,
        }
    }

    #[must_use]
    pub fn cluster_size(&self) -> u32 {
        match self {
            Self::Fat(fs) => fs.cluster_size(),
            Self::ExFat(fs) => fs.cluster_size() as u32,
        }
    }

    #[must_use]
    pub fn total_clusters(&self) -> u32 {
        match self {
            Self::Fat(fs) => fs.total_clusters(),
            Self::ExFat(fs) => fs.clusters(),
        }
    }

    /// The clusters marked taken and free since the mount.
    #[must_use]
    pub fn cluster_changes(&self) -> (u64, u64) {
        match self {
            Self::Fat(fs) => fs.cluster_changes(),
            Self::ExFat(fs) => fs.cluster_changes(),
        }
    }

    /// As `FileSystem::kept_free_count`; exFAT keeps none.
    ///
    /// # Errors
    ///
    /// `Error::Io` where the disk fails.
    pub fn kept_free_count(&self, note: &mut [u8]) -> Res<Option<(u32, u32)>, IO> {
        match self {
            Self::Fat(fs) => fs.kept_free_count(note),
            Self::ExFat(_) => Ok(None),
        }
    }

    /// As `FileSystem::keep_free_count`; exFAT keeps none (`false`).
    ///
    /// # Errors
    ///
    /// `Error::Io` where the disk fails.
    pub fn keep_free_count(&self, free: u32, note: &[u8]) -> Res<bool, IO> {
        match self {
            Self::Fat(fs) => fs.keep_free_count(free, note),
            Self::ExFat(_) => Ok(false),
        }
    }

    #[must_use]
    pub fn next_free_hint(&self) -> u32 {
        match self {
            Self::Fat(fs) => fs.next_free_hint(),
            Self::ExFat(_) => 0,
        }
    }

    #[must_use]
    pub fn free_count_kept(&self) -> bool {
        match self {
            Self::Fat(fs) => fs.free_count_kept(),
            Self::ExFat(_) => false,
        }
    }

    /// Whether the card said it was in use when it was mounted.
    #[must_use]
    pub fn marked_dirty(&self) -> bool {
        match self {
            Self::Fat(fs) => fs.marked_dirty(),
            Self::ExFat(fs) => fs.marked_dirty(),
        }
    }

    /// As `FileSystem::check_kept_places`, for tests; nothing on exFAT.
    #[cfg(debug_assertions)]
    pub fn check_kept_places(&self, on: bool) {
        if let Self::Fat(fs) = self {
            fs.check_kept_places(on);
        }
    }

    /// The volume's clusters, the free ones counted (all of the FAT or of
    /// the bitmap may be read).
    ///
    /// # Errors
    ///
    /// `Error::Io` where the disk fails.
    pub fn stats(&self) -> Res<Stats, IO> {
        match self {
            Self::Fat(fs) => {
                let stats = fs.stats()?;
                Ok(Stats {
                    cluster_size: stats.cluster_size(),
                    total_clusters: stats.total_clusters(),
                    free_clusters: stats.free_clusters(),
                })
            }
            Self::ExFat(fs) => Ok(Stats {
                cluster_size: fs.cluster_size() as u32,
                total_clusters: fs.clusters(),
                free_clusters: fs.count_free()?,
            }),
        }
    }

    /// An exFAT volume's bitmap, where it lies in one piece (as every
    /// formatter lays it): for a caller that counts the free clusters a
    /// part at a time. `None` for FAT, whose FAT tells them.
    #[must_use]
    pub fn free_map(&self) -> Option<FreeMap> {
        match self {
            Self::Fat(_) => None,
            Self::ExFat(fs) => Some(FreeMap {
                offset: fs.bitmap_start()?,
                clusters: fs.clusters(),
                cluster_size: fs.cluster_size() as u32,
            }),
        }
    }

    /// Writes back everything; an exFAT volume this mount marked in use
    /// is marked clean again.
    ///
    /// # Errors
    ///
    /// `Error::Io` where the disk fails.
    pub fn flush(&self) -> Res<(), IO> {
        match self {
            Self::Fat(fs) => fs.flush(),
            Self::ExFat(fs) => fs.flush(),
        }
    }

    /// After everything was written back: an exFAT volume this mount
    /// marked in use (before its first write) is marked clean again, so a
    /// computer checks the card only after a cut. A FAT volume stays as
    /// it is (marked in use until it is unmounted).
    ///
    /// # Errors
    ///
    /// `Error::Io` where the disk fails.
    pub fn rest(&self) -> Res<(), IO> {
        match self {
            Self::Fat(_) => Ok(()),
            Self::ExFat(fs) => fs.flush(),
        }
    }

    /// As `FileSystem::settle_claim`; nothing to do on exFAT.
    ///
    /// # Errors
    ///
    /// `Error::Io` where the disk fails.
    pub fn settle_claim(&self) -> Res<u32, IO> {
        match self {
            Self::Fat(fs) => fs.settle_claim(),
            Self::ExFat(_) => Ok(0),
        }
    }

    #[must_use]
    pub fn root_dir(&self) -> Dir<'a, IO> {
        match self {
            Self::Fat(fs) => Dir::Fat(fs.root_dir()),
            Self::ExFat(fs) => Dir::ExFat(fs.root_dir()),
        }
    }
}

/// A directory of either volume type, with the methods of [`crate::Dir`].
pub enum Dir<'a, IO: ReadWriteSeek> {
    Fat(crate::dir::Dir<'a, IO, Tp, Occ>),
    ExFat(XDir<'a, IO>),
}

impl<IO: ReadWriteSeek> Clone for Dir<'_, IO> {
    fn clone(&self) -> Self {
        match self {
            Self::Fat(dir) => Self::Fat(dir.clone()),
            Self::ExFat(dir) => Self::ExFat(dir.clone()),
        }
    }
}

/// The other type's handle given where this type's is needed.
fn mixed<T, E>() -> Result<T, Error<E>> {
    Err(Error::InvalidInput)
}

#[allow(clippy::missing_errors_doc)]
impl<'a, IO: ReadWriteSeek> Dir<'a, IO> {
    #[must_use]
    #[allow(clippy::iter_not_returning_iterator)]
    pub fn iter(&self) -> DirIter<'a, IO> {
        match self {
            Self::Fat(dir) => DirIter::Fat(dir.iter()),
            Self::ExFat(dir) => DirIter::ExFat(dir.iter()),
        }
    }

    pub fn iter_from(&self, offset: u64) -> Res<DirIter<'a, IO>, IO> {
        match self {
            Self::Fat(dir) => Ok(DirIter::Fat(dir.iter_from(offset)?)),
            Self::ExFat(dir) => Ok(DirIter::ExFat(dir.iter_at(offset))),
        }
    }

    #[must_use]
    pub fn note(&self) -> Option<u64> {
        match self {
            Self::Fat(dir) => dir.note(),
            Self::ExFat(dir) => dir.note(),
        }
    }

    #[must_use]
    pub fn entries_written(&self) -> u64 {
        match self {
            Self::Fat(dir) => dir.entries_written(),
            Self::ExFat(dir) => dir.entries_written(),
        }
    }

    pub fn set_note(&self, note: u64) {
        match self {
            Self::Fat(dir) => dir.set_note(note),
            Self::ExFat(dir) => dir.set_note(note),
        }
    }

    pub fn begins_entry(&self, offset: u64) -> Res<bool, IO> {
        match self {
            Self::Fat(dir) => dir.begins_entry(offset),
            Self::ExFat(dir) => dir.begins_entry(offset),
        }
    }

    /// File `name` of this directory (a name, not a path on exFAT).
    pub fn open_file(&self, name: &str) -> Res<File<'a, IO>, IO> {
        match self {
            Self::Fat(dir) => Ok(File::Fat(dir.open_file(name)?)),
            Self::ExFat(dir) => match dir.find(name)? {
                Some(entry) if !entry.is_dir() => Ok(File::ExFat(entry.to_file())),
                Some(_) => Err(Error::InvalidInput),
                None => Err(Error::NotFound),
            },
        }
    }

    /// Directory `name` of this directory (a name, not a path on exFAT).
    pub fn open_dir(&self, name: &str) -> Res<Self, IO> {
        match self {
            Self::Fat(dir) => Ok(Self::Fat(dir.open_dir(name)?)),
            Self::ExFat(dir) => match dir.find(name)? {
                Some(entry) if entry.is_dir() => Ok(Self::ExFat(entry.to_dir())),
                Some(_) => Err(Error::InvalidInput),
                None => Err(Error::NotFound),
            },
        }
    }

    pub fn create_file(&self, name: &str) -> Res<File<'a, IO>, IO> {
        match self {
            Self::Fat(dir) => Ok(File::Fat(dir.create_file(name)?)),
            Self::ExFat(dir) => Ok(File::ExFat(dir.create_file(name)?)),
        }
    }

    /// As `crate::Dir::create_file_with_cluster`. An exFAT file's set is
    /// written again when its size is: as [`Self::create_file`] there.
    pub fn create_file_with_cluster(&self, name: &str) -> Res<File<'a, IO>, IO> {
        match self {
            Self::Fat(dir) => Ok(File::Fat(dir.create_file_with_cluster(name)?)),
            Self::ExFat(dir) => Ok(File::ExFat(dir.create_file(name)?)),
        }
    }

    pub fn create_dir(&self, name: &str) -> Res<Self, IO> {
        match self {
            Self::Fat(dir) => Ok(Self::Fat(dir.create_dir(name)?)),
            Self::ExFat(dir) => Ok(Self::ExFat(dir.create_dir(name)?)),
        }
    }

    pub fn remove(&self, name: &str) -> Res<(), IO> {
        match self {
            Self::Fat(dir) => dir.remove(name),
            Self::ExFat(dir) => dir.remove(name),
        }
    }

    pub fn remove_found(&self, entry: &DirEntry<'a, IO>) -> Res<(), IO> {
        match (self, entry) {
            (Self::Fat(dir), DirEntry::Fat(entry)) => dir.remove_found(entry),
            (Self::ExFat(dir), DirEntry::ExFat(entry)) => dir.remove_found(entry, false),
            _ => mixed(),
        }
    }

    pub fn remove_found_emptied(&self, entry: &DirEntry<'a, IO>) -> Res<(), IO> {
        match (self, entry) {
            (Self::Fat(dir), DirEntry::Fat(entry)) => dir.remove_found_emptied(entry),
            (Self::ExFat(dir), DirEntry::ExFat(entry)) => dir.remove_found(entry, true),
            _ => mixed(),
        }
    }

    pub fn unlink(&self, name: &str) -> Res<(), IO> {
        match self {
            Self::Fat(dir) => dir.unlink(name),
            Self::ExFat(dir) => dir.unlink(name),
        }
    }

    pub fn unlink_found(&self, entry: &DirEntry<'a, IO>) -> Res<(), IO> {
        match (self, entry) {
            (Self::Fat(dir), DirEntry::Fat(entry)) => dir.unlink_found(entry),
            (Self::ExFat(dir), DirEntry::ExFat(entry)) => dir.unlink_found(entry),
            _ => mixed(),
        }
    }

    pub fn replace(&self, src_name: &str, dst_name: &str) -> Res<(), IO> {
        match self {
            Self::Fat(dir) => dir.replace(src_name, dst_name),
            Self::ExFat(dir) => dir.replace(src_name, dst_name),
        }
    }

    pub fn rename(&self, src_name: &str, dst_dir: &Dir<'a, IO>, dst_name: &str) -> Res<(), IO> {
        match (self, dst_dir) {
            (Self::Fat(dir), Self::Fat(to)) => dir.rename(src_name, to, dst_name),
            (Self::ExFat(dir), Self::ExFat(to)) => dir.rename(src_name, to, dst_name),
            _ => mixed(),
        }
    }

    pub fn rename_found(
        &self,
        entry: &DirEntry<'a, IO>,
        dst_dir: &Dir<'a, IO>,
        dst_name: &str,
    ) -> Res<(), IO> {
        match (self, entry, dst_dir) {
            (Self::Fat(dir), DirEntry::Fat(entry), Self::Fat(to)) => {
                dir.rename_found(entry, to, dst_name)
            }
            (Self::ExFat(dir), DirEntry::ExFat(entry), Self::ExFat(to)) => {
                dir.rename_found(entry, to, dst_name)
            }
            _ => mixed(),
        }
    }

    /// As `crate::Dir::point_up`; an exFAT directory names no parent.
    pub fn point_up(&self, parent: &Dir<'a, IO>) -> Res<(), IO> {
        match (self, parent) {
            (Self::Fat(dir), Self::Fat(parent)) => dir.point_up(parent),
            (Self::ExFat(_), Self::ExFat(_)) => Ok(()),
            _ => mixed(),
        }
    }
}

/// The entries of a directory.
pub enum DirIter<'a, IO: ReadWriteSeek> {
    Fat(crate::dir::DirIter<'a, IO, Tp, Occ>),
    ExFat(XIter<'a, IO>),
}

impl<'a, IO: ReadWriteSeek> Iterator for DirIter<'a, IO> {
    type Item = Res<DirEntry<'a, IO>, IO>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Fat(iter) => iter.next().map(|e| e.map(DirEntry::Fat)),
            Self::ExFat(iter) => iter.next().map(|e| e.map(DirEntry::ExFat)),
        }
    }
}

/// An entry of a directory, with the methods of [`crate::DirEntry`].
pub enum DirEntry<'a, IO: ReadWriteSeek> {
    Fat(crate::dir_entry::DirEntry<'a, IO, Tp, Occ>),
    ExFat(XEntry<'a, IO>),
}

impl<'a, IO: ReadWriteSeek> DirEntry<'a, IO> {
    /// Its short name. exFAT keeps none: its name where that is one a
    /// short name could be (12 ASCII letters at most), so that a caller
    /// finds the files it named so by them on either type; else empty.
    #[must_use]
    pub fn short_file_name_as_bytes(&self) -> &[u8] {
        match self {
            Self::Fat(entry) => entry.short_file_name_as_bytes(),
            Self::ExFat(entry) => entry.short_name(),
        }
    }

    /// Its short name (see [`Self::short_file_name_as_bytes`]).
    #[must_use]
    pub fn short_file_name(&self) -> String {
        match self {
            Self::Fat(entry) => entry.short_file_name(),
            Self::ExFat(entry) => String::from_utf8_lossy(entry.short_name()).into_owned(),
        }
    }

    #[must_use]
    pub fn long_file_name_as_ucs2_units(&self) -> Option<&[u16]> {
        match self {
            Self::Fat(entry) => entry.long_file_name_as_ucs2_units(),
            Self::ExFat(entry) => Some(entry.name_units()),
        }
    }

    #[must_use]
    pub fn file_name(&self) -> String {
        match self {
            Self::Fat(entry) => entry.file_name(),
            Self::ExFat(entry) => entry.file_name(),
        }
    }

    #[must_use]
    pub fn attributes(&self) -> FileAttributes {
        match self {
            Self::Fat(entry) => entry.attributes(),
            Self::ExFat(entry) => entry.attributes(),
        }
    }

    #[must_use]
    pub fn is_dir(&self) -> bool {
        match self {
            Self::Fat(entry) => entry.is_dir(),
            Self::ExFat(entry) => entry.is_dir(),
        }
    }

    #[must_use]
    pub fn is_file(&self) -> bool {
        !self.is_dir()
    }

    #[must_use]
    pub fn first_cluster(&self) -> Option<u32> {
        match self {
            Self::Fat(entry) => entry.first_cluster(),
            Self::ExFat(entry) => entry.first_cluster(),
        }
    }

    #[must_use]
    pub fn to_file(&self) -> File<'a, IO> {
        match self {
            Self::Fat(entry) => File::Fat(entry.to_file()),
            Self::ExFat(entry) => File::ExFat(entry.to_file()),
        }
    }

    #[must_use]
    pub fn to_dir(&self) -> Dir<'a, IO> {
        match self {
            Self::Fat(entry) => Dir::Fat(entry.to_dir()),
            Self::ExFat(entry) => Dir::ExFat(entry.to_dir()),
        }
    }

    #[must_use]
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> u64 {
        match self {
            Self::Fat(entry) => entry.len(),
            Self::ExFat(entry) => entry.len(),
        }
    }

    #[must_use]
    pub fn modified(&self) -> DateTime {
        match self {
            Self::Fat(entry) => entry.modified(),
            Self::ExFat(entry) => entry.modified(),
        }
    }

    #[must_use]
    pub fn offset_range(&self) -> (u64, u64) {
        match self {
            Self::Fat(entry) => entry.offset_range(),
            Self::ExFat(entry) => entry.offset_range(),
        }
    }

    #[must_use]
    pub fn mark(&self) -> Option<u32> {
        match self {
            Self::Fat(entry) => entry.mark(),
            Self::ExFat(entry) => entry.mark(),
        }
    }
}

/// A file of either volume type, with the methods of [`crate::File`].
pub enum File<'a, IO: ReadWriteSeek> {
    Fat(crate::file::File<'a, IO, Tp, Occ>),
    ExFat(XFile<'a, IO>),
}

#[allow(clippy::missing_errors_doc)]
impl<IO: ReadWriteSeek> File<'_, IO> {
    /// As `crate::File::reserve_ahead`. An exFAT file takes the cluster
    /// after its last one where it is free, one bit of the bitmap each:
    /// nothing is taken ahead.
    pub fn reserve_ahead(&mut self, clusters: u32) {
        if let Self::Fat(file) = self {
            file.reserve_ahead(clusters);
        }
    }

    pub fn maybe_ahead(&mut self) {
        if let Self::Fat(file) = self {
            file.maybe_ahead();
        }
    }

    pub fn trim_ahead(&mut self) -> Res<(), IO> {
        match self {
            Self::Fat(file) => file.trim_ahead(),
            Self::ExFat(_) => Ok(()),
        }
    }

    pub fn truncate(&mut self) -> Res<(), IO> {
        match self {
            Self::Fat(file) => file.truncate(),
            Self::ExFat(file) => file.truncate(),
        }
    }

    pub fn set_mark(&mut self, mark: u32) {
        match self {
            Self::Fat(file) => file.set_mark(mark),
            Self::ExFat(file) => file.set_mark(mark),
        }
    }

    pub fn set_modified(&mut self, date_time: DateTime) {
        match self {
            Self::Fat(file) => file.set_modified(date_time),
            Self::ExFat(file) => file.set_modified(date_time),
        }
    }

    pub fn set_attributes(&mut self, attrs: FileAttributes) {
        match self {
            Self::Fat(file) => file.set_attributes(attrs),
            Self::ExFat(file) => file.set_attributes(attrs),
        }
    }
}

impl<IO: ReadWriteSeek> IoBase for File<'_, IO> {
    type Error = Error<IO::Error>;
}

impl<IO: ReadWriteSeek> Read for File<'_, IO> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        match self {
            Self::Fat(file) => file.read(buf),
            Self::ExFat(file) => file.read(buf),
        }
    }
}

impl<IO: ReadWriteSeek> Write for File<'_, IO> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        match self {
            Self::Fat(file) => file.write(buf),
            Self::ExFat(file) => file.write(buf),
        }
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        match self {
            Self::Fat(file) => file.flush(),
            Self::ExFat(file) => file.flush(),
        }
    }
}

impl<IO: ReadWriteSeek> Seek for File<'_, IO> {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, Self::Error> {
        match self {
            Self::Fat(file) => file.seek(pos),
            Self::ExFat(file) => file.seek(pos),
        }
    }
}
