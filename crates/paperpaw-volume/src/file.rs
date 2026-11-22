use core::convert::TryFrom;

use crate::dir_entry::{DirEntryEditor, FileAttributes};
use crate::error::Error;
use crate::fs::{FileSystem, ReadWriteSeek, Referrer};
use crate::io::{IoBase, Read, Seek, SeekFrom, Write};
use crate::time::{Date, DateTime, TimeProvider};

const MAX_FILE_SIZE: u32 = u32::MAX;
// Modified for PaperPaw: added. The bytes of a directory at most: FAT numbers its entries in 16
// bits (65,536 of them). Its size is not in an entry; its chain says it, and a chain that runs in
// a loop has no end.
const MAX_DIR_SIZE: u32 = 65_536 * 32;

/// A FAT filesystem file object used for reading and writing data.
///
/// This struct is created by the `open_file` or `create_file` methods on `Dir`.
pub struct File<'a, IO: ReadWriteSeek, TP, OCC> {
    // Note first_cluster is None if file is empty
    first_cluster: Option<u32>,
    // Note: if offset points between clusters current_cluster is the previous cluster
    current_cluster: Option<u32>,
    // current position in this file
    offset: u32,
    // file dir entry editor - None for root dir
    entry: Option<DirEntryEditor>,
    // file-system reference
    fs: &'a FileSystem<IO, TP, OCC>,
    // Modified for PaperPaw: the clusters taken beyond the one needed when the file grows (0: one at a
    // time, as before); see `reserve_ahead`.
    ahead: u32,
    // Modified for PaperPaw: clusters were taken beyond the file's end and may still be there; see
    // `trim_ahead`.
    ahead_taken: bool,
}

/// An extent containing a file's data on disk.
///
/// This is created by the `extents` method on `File`, and represents
/// a byte range on the disk that contains a file's data. All values
/// are in bytes.
#[derive(Clone, Debug)]
pub struct Extent {
    pub offset: u64,
    pub size: u32,
}

impl<'a, IO: ReadWriteSeek, TP, OCC> File<'a, IO, TP, OCC> {
    pub(crate) fn new(
        first_cluster: Option<u32>,
        entry: Option<DirEntryEditor>,
        fs: &'a FileSystem<IO, TP, OCC>,
    ) -> Self {
        File {
            first_cluster,
            entry,
            fs,
            current_cluster: None, // cluster before first one
            offset: 0,
            ahead: 0,
            ahead_taken: false,
        }
    }

    // Modified for PaperPaw: added.
    /// Makes a file that grows past its last cluster take `clusters` more than that one, in a row where
    /// they are free, in one go (0: one at a time). The FAT is written once for the run instead of once
    /// for each cluster; the clusters beyond what is written stay linked after it, and are used by the
    /// writes that follow. [`trim_ahead`](Self::trim_ahead) gives back what is left of them. Not for a
    /// directory.
    pub fn reserve_ahead(&mut self, clusters: u32) {
        self.ahead = clusters.min(crate::table::MAX_RUN - 1);
    }

    // Modified for PaperPaw: added.
    /// Makes [`trim_ahead`](Self::trim_ahead) look at the end of the chain once: a cut may have left
    /// clusters taken ahead there (the size in the entry is older), and a write that ends inside the
    /// last cluster in use would not meet them. Not for a directory.
    pub fn maybe_ahead(&mut self) {
        self.ahead_taken = true;
    }

    // Modified for PaperPaw: added.
    /// Whether [`trim_ahead`](Self::trim_ahead) has something to do.
    #[must_use]
    pub fn ahead_taken(&self) -> bool {
        self.ahead_taken
    }

    // Modified for PaperPaw: added.
    /// Frees the clusters this file took beyond its end (see [`reserve_ahead`](Self::reserve_ahead)),
    /// and makes the cluster of its last byte the end of its chain. The position is where the file
    /// ends afterwards. Cut off before this, the clusters stay linked after the last one used, with
    /// the size in the file's entry unchanged: they are read as no data; the next write takes them,
    /// and removing the file frees them.
    ///
    /// # Errors
    ///
    /// `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn trim_ahead(&mut self) -> Result<(), Error<IO::Error>> {
        if !self.ahead_taken {
            return Ok(());
        }
        self.seek(SeekFrom::End(0))?;
        if let Some(cluster) = self.current_cluster {
            self.fs.truncate_cluster_chain_run(cluster)?;
        }
        self.ahead_taken = false;
        Ok(())
    }

    /// Truncate file in current position.
    ///
    /// # Errors
    ///
    /// `Error::Io` will be returned if the underlying storage object returned an I/O error.
    ///
    /// # Panics
    ///
    /// Will panic if this is the root directory.
    pub fn truncate(&mut self) -> Result<(), Error<IO::Error>> {
        trace!("File::truncate");
        if let Some(ref mut e) = self.entry {
            e.set_size(self.offset);
            if self.offset == 0 {
                e.set_first_cluster(None, self.fs.fat_type());
            }
        } else {
            // Note: we cannot handle this case because there is no size field
            panic!("Trying to truncate a file without an entry");
        }
        if let Some(current_cluster) = self.current_cluster {
            // current cluster is none only if offset is 0
            debug_assert!(self.offset > 0);
            self.fs.truncate_cluster_chain(current_cluster)
        } else {
            debug_assert_eq!(self.offset, 0);
            if let Some(n) = self.first_cluster {
                self.fs.free_cluster_chain(n)?;
                self.first_cluster = None;
            }
            Ok(())
        }
    }

    /// Get the extents of a file on disk.
    ///
    /// This returns an iterator over the byte ranges on-disk occupied by
    /// this file.
    pub fn extents(&mut self) -> impl Iterator<Item = Result<Extent, Error<IO::Error>>> + 'a {
        let fs = self.fs;
        let cluster_size = fs.cluster_size();
        let Some(mut bytes_left) = self.size() else {
            return None.into_iter().flatten();
        };
        let Some(first) = self.first_cluster else {
            return None.into_iter().flatten();
        };

        Some(
            core::iter::once(Ok(first))
                .chain(fs.cluster_iter(first))
                .map(move |cluster_err| match cluster_err {
                    Ok(cluster) => {
                        let size = cluster_size.min(bytes_left);
                        bytes_left -= size;
                        Ok(Extent {
                            offset: fs.offset_from_cluster(cluster)?,
                            size,
                        })
                    }
                    Err(e) => Err(e),
                }),
        )
        .into_iter()
        .flatten()
    }

    pub(crate) fn abs_pos(&self) -> Option<u64> {
        // Returns current position relative to filesystem start
        // Note: when between clusters it returns position after previous cluster
        match self.current_cluster {
            Some(n) => {
                let cluster_size = self.fs.cluster_size();
                let offset_mod_cluster_size = self.offset % cluster_size;
                let offset_in_cluster = if offset_mod_cluster_size == 0 {
                    // position points between clusters - we are returning previous cluster so
                    // offset must be set to the cluster size
                    cluster_size
                } else {
                    offset_mod_cluster_size
                };
                // `None` where it is none of the volume's: the read or write that came to it
                // failed already.
                let offset_in_fs =
                    self.fs.offset_from_cluster(n).ok()? + u64::from(offset_in_cluster);
                Some(offset_in_fs)
            }
            None => None,
        }
    }

    fn flush_dir_entry(&mut self) -> Result<(), Error<IO::Error>> {
        if let Some(ref mut e) = self.entry {
            e.flush(self.fs)?;
        }
        Ok(())
    }

    /// Sets date and time of creation for this file.
    ///
    /// Note: it is set to a value from the `TimeProvider` when creating a file.
    pub fn set_created(&mut self, date_time: DateTime) {
        if let Some(ref mut e) = self.entry {
            e.set_created(date_time);
        }
    }

    /// Sets date of last access for this file.
    ///
    /// Note: it is overwritten by a value from the `TimeProvider` on every file read operation.
    pub fn set_accessed(&mut self, date: Date) {
        if let Some(ref mut e) = self.entry {
            e.set_accessed(date);
        }
    }

    /// Sets date and time of last modification for this file.
    ///
    /// Note: it is overwritten by a value from the `TimeProvider` on every file write operation.
    pub fn set_modified(&mut self, date_time: DateTime) {
        if let Some(ref mut e) = self.entry {
            e.set_modified(date_time);
        }
    }

    // Modified for PaperPaw: added.
    /// Gives this file a mark of the caller's own, kept in its creation time instead (whose
    /// hundredths of a second tell it from a time). Written with the directory entry, when the file
    /// is flushed or dropped; read with `DirEntry::mark`. Kept when the entry is renamed; not by
    /// `Dir::replace` (see there).
    pub fn set_mark(&mut self, mark: u32) {
        if let Some(ref mut e) = self.entry {
            e.set_mark(mark);
        }
    }

    // Modified for PaperPaw: `chain_at` and `seek_from` added.
    /// Where the position stands in the cluster chain: how many clusters from the start the
    /// cluster holding the byte before it is (1 for the first) and that cluster. `None` at the
    /// start or where the file has no cluster. `seek_from` takes it.
    pub(crate) fn chain_at(&self) -> Option<(u32, u32)> {
        let cluster = self.current_cluster?;
        let n = self.fs.clusters_from_bytes(u64::from(self.offset));
        (n > 0).then_some((n, cluster))
    }

    /// Seeks to `offset` from the start, walking the chain on from `at` (a place `chain_at` gave
    /// for this very chain) where that is ahead of the position and not past `offset`: a seek
    /// far into a long chain does not read the FAT from its first cluster again.
    pub(crate) fn seek_from(
        &mut self,
        offset: u64,
        at: Option<(u32, u32)>,
    ) -> Result<u64, Error<IO::Error>> {
        if let (Some((n, cluster)), Some(_)) = (at, self.first_cluster) {
            let ahead = n > self.fs.clusters_from_bytes(u64::from(self.offset));
            let bytes = self.fs.bytes_from_clusters(n);
            let inside = !matches!(self.size(), Some(size) if bytes > u64::from(size));
            if ahead && inside && n <= self.fs.clusters_from_bytes(offset) {
                if let Ok(bytes) = u32::try_from(bytes) {
                    #[cfg(debug_assertions)]
                    self.debug_check_chain_at(n, cluster);
                    self.offset = bytes;
                    self.current_cluster = Some(cluster);
                }
            }
        }
        self.seek(SeekFrom::Start(offset))
    }

    /// In debug builds, where the file system is told to (`FileSystem::check_kept_places`): that
    /// the `n`th cluster of the chain (1 for the first) is `cluster`, as a place kept for it says
    /// (see `DirNote`).
    #[cfg(debug_assertions)]
    fn debug_check_chain_at(&self, n: u32, cluster: u32) {
        let Some(first) = self.first_cluster.filter(|_| self.fs.checks_kept_places()) else {
            return;
        };
        let mut walked = first;
        let mut iter = self.fs.cluster_iter(first);
        for _ in 1..n {
            match iter.next() {
                Some(Ok(next)) => walked = next,
                // What the disk does not give cannot be told here.
                _ => return,
            }
        }
        debug_assert_eq!(
            walked, cluster,
            "the place kept for cluster {} of the chain from {}",
            n, first
        );
    }

    /// Returns the attributes of this file.
    ///
    /// The root directory, which has no directory entry, reports `FileAttributes::DIRECTORY`.
    #[must_use]
    pub fn attributes(&self) -> FileAttributes {
        match self.entry {
            Some(ref e) => e.inner().attributes(),
            None => FileAttributes::DIRECTORY,
        }
    }

    /// Sets the read-only, hidden, system and archive attributes of this file.
    ///
    /// The other bits of `attrs` are ignored: whether the entry is a directory or a volume label
    /// cannot be changed. The new attributes are written with the directory entry, when the file is
    /// flushed or dropped. Nothing happens for the root directory, which has no directory entry.
    pub fn set_attributes(&mut self, attrs: FileAttributes) {
        if let Some(ref mut e) = self.entry {
            let settable = FileAttributes::READ_ONLY
                | FileAttributes::HIDDEN
                | FileAttributes::SYSTEM
                | FileAttributes::ARCHIVE;
            let kept = e.inner().attributes() & !settable;
            e.set_attributes(kept | (attrs & settable));
        }
    }

    fn size(&self) -> Option<u32> {
        match self.entry {
            Some(ref e) => e.inner().size(),
            None => None,
        }
    }

    fn is_dir(&self) -> bool {
        match self.entry {
            Some(ref e) => e.inner().is_dir(),
            None => true, // root directory
        }
    }

    fn bytes_left_in_file(&self) -> Option<usize> {
        // Note: seeking beyond end of file is not allowed so overflow is impossible
        self.size().map(|s| (s - self.offset) as usize)
    }

    fn set_first_cluster(&mut self, cluster: u32) {
        self.first_cluster = Some(cluster);
        if let Some(ref mut e) = self.entry {
            e.set_first_cluster(self.first_cluster, self.fs.fat_type());
        }
    }

    pub(crate) fn first_cluster(&self) -> Option<u32> {
        self.first_cluster
    }

    fn flush(&mut self) -> Result<(), Error<IO::Error>> {
        self.flush_dir_entry()?;
        let mut disk = self.fs.disk.borrow_mut();
        disk.flush()?;
        Ok(())
    }

    pub(crate) fn is_root_dir(&self) -> bool {
        self.entry.is_none()
    }
}

impl<IO: ReadWriteSeek, TP: TimeProvider, OCC> File<'_, IO, TP, OCC> {
    fn update_dir_entry_after_write(&mut self) {
        let offset = self.offset;
        if let Some(ref mut e) = self.entry {
            let now = self.fs.options.time_provider.get_current_date_time();
            e.set_modified(now);
            if e.inner().size().map_or(false, |s| offset > s) {
                e.set_size(offset);
            }
        }
    }
}

impl<IO: ReadWriteSeek, TP, OCC> Drop for File<'_, IO, TP, OCC> {
    fn drop(&mut self) {
        // Modified for PaperPaw: what was taken ahead of an append is given back first.
        if let Err(err) = self.trim_ahead() {
            error!("trim failed {:?}", err);
        }
        // Modified for PaperPaw: its entry is written; the disk is not flushed for it. A disk
        // that keeps changed blocks writes them back in the order they were changed, when its
        // owner says so (`FatStorage` does when a call that changes the card returns): flushed
        // here, at the end of every step of a change, each step was a write of its own.
        if let Err(err) = self.flush_dir_entry() {
            error!("flush failed {:?}", err);
        }
    }
}

// Note: derive cannot be used because of invalid bounds. See: https://github.com/rust-lang/rust/issues/26925
impl<IO: ReadWriteSeek, TP, OCC> Clone for File<'_, IO, TP, OCC> {
    fn clone(&self) -> Self {
        File {
            first_cluster: self.first_cluster,
            current_cluster: self.current_cluster,
            offset: self.offset,
            entry: self.entry.clone(),
            fs: self.fs,
            // A copy takes clusters as one at a time and has none to give back.
            ahead: 0,
            ahead_taken: false,
        }
    }
}

impl<IO: ReadWriteSeek, TP, OCC> IoBase for File<'_, IO, TP, OCC> {
    type Error = Error<IO::Error>;
}

impl<IO: ReadWriteSeek, TP: TimeProvider, OCC> Read for File<'_, IO, TP, OCC> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        trace!("File::read");
        let cluster_size = self.fs.cluster_size();
        let current_cluster_opt = if self.offset % cluster_size == 0 {
            // next cluster
            match self.current_cluster {
                None => self.first_cluster,
                Some(n) => {
                    let r = self.fs.cluster_iter(n).next();
                    match r {
                        Some(Err(err)) => return Err(err),
                        Some(Ok(n)) => Some(n),
                        None => None,
                    }
                }
            }
        } else {
            self.current_cluster
        };
        let Some(current_cluster) = current_cluster_opt else {
            return Ok(0);
        };
        // Modified for PaperPaw: a directory whose chain goes on past its largest size is damaged
        // (one that ran in a loop was read for ever).
        if self.is_dir() && self.offset >= MAX_DIR_SIZE {
            return Err(Error::CorruptedFileSystem);
        }
        let offset_in_cluster = self.offset % cluster_size;
        let bytes_left_in_cluster = (cluster_size - offset_in_cluster) as usize;
        let bytes_left_in_file = self.bytes_left_in_file().unwrap_or(bytes_left_in_cluster);
        let read_size = buf.len().min(bytes_left_in_cluster).min(bytes_left_in_file);
        if read_size == 0 {
            return Ok(0);
        }
        trace!("read {} bytes in cluster {}", read_size, current_cluster);
        let offset_in_fs =
            self.fs.offset_from_cluster(current_cluster)? + u64::from(offset_in_cluster);
        let read_bytes = {
            let mut disk = self.fs.disk.borrow_mut();
            disk.seek(SeekFrom::Start(offset_in_fs))?;
            disk.read(&mut buf[..read_size])?
        };
        if read_bytes == 0 {
            return Ok(0);
        }
        self.offset += read_bytes as u32;
        self.current_cluster = Some(current_cluster);

        if let Some(ref mut e) = self.entry {
            if self.fs.options.update_accessed_date {
                let now = self.fs.options.time_provider.get_current_date();
                e.set_accessed(now);
            }
        }
        Ok(read_bytes)
    }
}

#[cfg(feature = "std")]
impl<IO: ReadWriteSeek, TP: TimeProvider, OCC> std::io::Read for File<'_, IO, TP, OCC>
where
    std::io::Error: From<Error<IO::Error>>,
{
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        Ok(Read::read(self, buf)?)
    }
}

impl<IO: ReadWriteSeek, TP: TimeProvider, OCC> Write for File<'_, IO, TP, OCC> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        trace!("File::write");
        let cluster_size = self.fs.cluster_size();
        let offset_in_cluster = self.offset % cluster_size;
        let bytes_left_in_cluster = (cluster_size - offset_in_cluster) as usize;
        let bytes_left_until_max_file_size = (MAX_FILE_SIZE - self.offset) as usize;
        let write_size = buf
            .len()
            .min(bytes_left_in_cluster)
            .min(bytes_left_until_max_file_size);
        // Exit early if we are going to write no data
        if write_size == 0 {
            return Ok(0);
        }
        // Modified for PaperPaw: a directory does not grow past its largest size.
        if self.is_dir() && self.offset >= MAX_DIR_SIZE {
            return Err(Error::NotEnoughSpace);
        }
        // Modified for PaperPaw: the volume is not marked dirty for a file's data (see
        // `FsIoAdapter::write`).
        // Get cluster for write possibly allocating new one
        let current_cluster = if self.offset % cluster_size == 0 {
            // next cluster
            let next_cluster = match self.current_cluster {
                None => self.first_cluster,
                Some(n) => {
                    let r = self.fs.cluster_iter(n).next();
                    match r {
                        Some(Err(err)) => return Err(err),
                        Some(Ok(n)) => Some(n),
                        None => None,
                    }
                }
            };
            if let Some(n) = next_cluster {
                n
            } else {
                // end of chain reached - allocate new cluster
                // Modified for PaperPaw: a file (not a directory) that was asked to may take a run.
                let new_cluster = if self.is_dir() {
                    let referrer = self
                        .current_cluster
                        .map_or(Referrer::Unknown, Referrer::Link);
                    self.fs.alloc_cluster(self.current_cluster, referrer)?
                } else {
                    let entry = self
                        .entry
                        .as_ref()
                        .map_or(Referrer::Unknown, |e| Referrer::NewEntry(e.pos()));
                    let (first, count) =
                        self.fs
                            .alloc_clusters(self.current_cluster, 1 + self.ahead, entry)?;
                    if count > 1 {
                        self.ahead_taken = true;
                    }
                    first
                };
                trace!("allocated cluster {}", new_cluster);
                if self.first_cluster.is_none() {
                    self.set_first_cluster(new_cluster);
                    // Modified for PaperPaw: the entry gets its first cluster at once, not when
                    // the file is flushed. Cut off before that (power lost), the clusters of a
                    // new file were taken by no file (#238); now they are the file's, with the
                    // size it had, and are freed with it or used by the next write.
                    self.flush_dir_entry()?;
                }
                new_cluster
            }
        } else {
            // self.current_cluster should be a valid cluster
            match self.current_cluster {
                Some(n) => n,
                None => panic!("Offset inside cluster but no cluster allocated"),
            }
        };
        trace!("write {} bytes in cluster {}", write_size, current_cluster);
        let offset_in_fs =
            self.fs.offset_from_cluster(current_cluster)? + u64::from(offset_in_cluster);
        let written_bytes = {
            let mut disk = self.fs.disk.borrow_mut();
            disk.seek(SeekFrom::Start(offset_in_fs))?;
            disk.write(&buf[..write_size])?
        };
        if written_bytes == 0 {
            return Ok(0);
        }
        // some bytes were writter - update position and optionally size
        self.offset += written_bytes as u32;
        self.current_cluster = Some(current_cluster);
        self.update_dir_entry_after_write();
        Ok(written_bytes)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Self::flush(self)
    }
}

#[cfg(feature = "std")]
impl<IO: ReadWriteSeek, TP: TimeProvider, OCC> std::io::Write for File<'_, IO, TP, OCC>
where
    std::io::Error: From<Error<IO::Error>>,
{
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Ok(Write::write(self, buf)?)
    }

    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        Ok(Write::write_all(self, buf)?)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(Write::flush(self)?)
    }
}

impl<IO: ReadWriteSeek, TP, OCC> Seek for File<'_, IO, TP, OCC> {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, Self::Error> {
        trace!("File::seek");
        let size_opt = self.size();
        let new_offset_opt: Option<u32> = match pos {
            SeekFrom::Current(x) => i64::from(self.offset)
                .checked_add(x)
                .and_then(|n| u32::try_from(n).ok()),
            SeekFrom::Start(x) => u32::try_from(x).ok(),
            SeekFrom::End(o) => size_opt
                .and_then(|s| i64::from(s).checked_add(o))
                .and_then(|n| u32::try_from(n).ok()),
        };
        let Some(mut new_offset) = new_offset_opt else {
            error!("Invalid seek offset");
            return Err(Error::InvalidInput);
        };
        if let Some(size) = size_opt {
            if new_offset > size {
                warn!("Seek beyond the end of the file");
                new_offset = size;
            }
        }
        trace!(
            "file seek {} -> {} - entry {:?}",
            self.offset, new_offset, self.entry
        );
        if new_offset == self.offset {
            // position is the same - nothing to do
            return Ok(u64::from(self.offset));
        }
        let new_offset_in_clusters = self.fs.clusters_from_bytes(u64::from(new_offset));
        let old_offset_in_clusters = self.fs.clusters_from_bytes(u64::from(self.offset));
        let new_cluster = if new_offset == 0 {
            None
        } else if new_offset_in_clusters == old_offset_in_clusters {
            self.current_cluster
        } else if let Some(first_cluster) = self.first_cluster {
            // calculate number of clusters to skip
            // return the previous cluster if the offset points to the cluster boundary
            // Note: new_offset_in_clusters cannot be 0 here because new_offset is not 0
            debug_assert!(new_offset_in_clusters > 0);
            // Modified for PaperPaw: forward, the chain is walked on from the current cluster (the
            // `old_offset_in_clusters`th, counted from 1), as reading on would walk it, not from the
            // first one again.
            let (start_cluster, start_in_clusters) = match self.current_cluster {
                Some(n)
                    if old_offset_in_clusters > 0
                        && new_offset_in_clusters > old_offset_in_clusters =>
                {
                    (n, old_offset_in_clusters)
                }
                _ => (first_cluster, 1),
            };
            let clusters_to_skip = new_offset_in_clusters - start_in_clusters;
            let mut cluster = start_cluster;
            let mut iter = self.fs.cluster_iter(start_cluster);
            for i in 0..clusters_to_skip {
                cluster = if let Some(r) = iter.next() {
                    r?
                } else {
                    // cluster chain ends before the new position - seek to the end of the last cluster
                    new_offset = self.fs.bytes_from_clusters(start_in_clusters + i) as u32;
                    break;
                };
            }
            Some(cluster)
        } else {
            // empty file - always seek to 0
            new_offset = 0;
            None
        };
        self.offset = new_offset;
        self.current_cluster = new_cluster;
        Ok(u64::from(self.offset))
    }
}

#[cfg(feature = "std")]
impl<IO: ReadWriteSeek, TP: TimeProvider, OCC> std::io::Seek for File<'_, IO, TP, OCC>
where
    std::io::Error: From<Error<IO::Error>>,
{
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        Ok(Seek::seek(self, pos.into())?)
    }
}
