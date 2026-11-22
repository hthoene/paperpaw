use alloc::vec::Vec;
use core::num;
use core::str;
use core::{iter, slice};

use crate::dir_entry::{
    DIR_ENTRY_SIZE, DirEntry, DirEntryData, DirFileEntryData, DirLfnEntryData, FileAttributes,
    ShortName,
};
use crate::dir_entry::{LFN_ENTRY_LAST_FLAG, LFN_PART_LEN};
use crate::dir_entry::{SFN_PADDING, SFN_SIZE};
use crate::error::{Error, IoError};
use crate::file::File;
use crate::fs::{DiskSlice, FileSystem, FsIoAdapter, OemCpConverter, ReadWriteSeek, Referrer};
use crate::io::{self, IoBase, Read, Seek, SeekFrom, Write};
use crate::names::*;
use crate::time::TimeProvider;

pub(crate) enum DirRawStream<'a, IO: ReadWriteSeek, TP, OCC> {
    File(File<'a, IO, TP, OCC>),
    Root(DiskSlice<FsIoAdapter<'a, IO, TP, OCC>, FsIoAdapter<'a, IO, TP, OCC>>),
}

impl<IO: ReadWriteSeek, TP, OCC> DirRawStream<'_, IO, TP, OCC> {
    fn abs_pos(&self) -> Option<u64> {
        match self {
            DirRawStream::File(file) => file.abs_pos(),
            DirRawStream::Root(slice) => Some(slice.abs_pos()),
        }
    }

    fn first_cluster(&self) -> Option<u32> {
        match self {
            DirRawStream::File(file) => file.first_cluster(),
            DirRawStream::Root(_) => None,
        }
    }

    pub(crate) fn is_root_dir(&self) -> bool {
        match self {
            DirRawStream::File(file) => file.is_root_dir(),
            DirRawStream::Root(_) => true,
        }
    }
}

// Modified for PaperPaw: `chain_at` and `seek_from` added (see `File::seek_from`).
impl<IO: ReadWriteSeek, TP, OCC> DirRawStream<'_, IO, TP, OCC> {
    fn chain_at(&self) -> Option<(u32, u32)> {
        match self {
            DirRawStream::File(file) => file.chain_at(),
            DirRawStream::Root(_) => None,
        }
    }

    fn seek_from(&mut self, offset: u64, at: Option<(u32, u32)>) -> Result<u64, Error<IO::Error>> {
        match self {
            DirRawStream::File(file) => file.seek_from(offset, at),
            DirRawStream::Root(raw) => raw.seek(SeekFrom::Start(offset)),
        }
    }
}

// Note: derive cannot be used because of invalid bounds. See: https://github.com/rust-lang/rust/issues/26925
impl<IO: ReadWriteSeek, TP, OCC> Clone for DirRawStream<'_, IO, TP, OCC> {
    fn clone(&self) -> Self {
        match self {
            DirRawStream::File(file) => DirRawStream::File(file.clone()),
            DirRawStream::Root(raw) => DirRawStream::Root(raw.clone()),
        }
    }
}

impl<IO: ReadWriteSeek, TP, OCC> IoBase for DirRawStream<'_, IO, TP, OCC> {
    type Error = Error<IO::Error>;
}

impl<IO: ReadWriteSeek, TP: TimeProvider, OCC> Read for DirRawStream<'_, IO, TP, OCC> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        match self {
            DirRawStream::File(file) => file.read(buf),
            DirRawStream::Root(raw) => raw.read(buf),
        }
    }
}

impl<IO: ReadWriteSeek, TP: TimeProvider, OCC> Write for DirRawStream<'_, IO, TP, OCC> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        match self {
            DirRawStream::File(file) => file.write(buf),
            DirRawStream::Root(raw) => raw.write(buf),
        }
    }
    fn flush(&mut self) -> Result<(), Self::Error> {
        match self {
            DirRawStream::File(file) => file.flush(),
            DirRawStream::Root(raw) => raw.flush(),
        }
    }
}

impl<IO: ReadWriteSeek, TP, OCC> Seek for DirRawStream<'_, IO, TP, OCC> {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, Self::Error> {
        match self {
            DirRawStream::File(file) => file.seek(pos),
            DirRawStream::Root(raw) => raw.seek(pos),
        }
    }
}

fn split_path(path: &str) -> (&str, Option<&str>) {
    let trimmed_path = path.trim_matches('/');
    trimmed_path.find('/').map_or((trimmed_path, None), |n| {
        (&trimmed_path[..n], Some(&trimmed_path[n + 1..]))
    })
}

enum DirEntryOrShortName<'a, IO: ReadWriteSeek, TP, OCC> {
    DirEntry(DirEntry<'a, IO, TP, OCC>),
    ShortName([u8; SFN_SIZE]),
}

/// A FAT filesystem directory.
///
/// This struct is created by the `open_dir` or `create_dir` methods on `Dir`.
/// The root directory is returned by the `root_dir` method on `FileSystem`.
pub struct Dir<'a, IO: ReadWriteSeek, TP, OCC> {
    stream: DirRawStream<'a, IO, TP, OCC>,
    fs: &'a FileSystem<IO, TP, OCC>,
}

impl<'a, IO: ReadWriteSeek, TP, OCC> Dir<'a, IO, TP, OCC> {
    pub(crate) fn new(
        stream: DirRawStream<'a, IO, TP, OCC>,
        fs: &'a FileSystem<IO, TP, OCC>,
    ) -> Self {
        Dir { stream, fs }
    }

    /// Creates directory entries iterator.
    #[must_use]
    #[allow(clippy::iter_not_returning_iterator)]
    pub fn iter(&self) -> DirIter<'a, IO, TP, OCC> {
        DirIter::new(self.stream.clone(), self.fs, true)
    }

    // Modified for PaperPaw: `iter_from`, `note`, `entries_written` and `set_note` added.

    /// Creates directory entries iterator from byte `offset` of the directory on, where an entry
    /// (or a deleted one) begins: as `DirEntry::offset_range` says where one ends.
    ///
    /// # Errors
    ///
    /// `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn iter_from(&self, offset: u64) -> Result<DirIter<'a, IO, TP, OCC>, Error<IO::Error>> {
        let mut stream = self.stream.clone();
        // From where the last such seek in this directory ended, while it is noted: the chain of a
        // directory of thousands of entries is long, and seeking from its first cluster reads the
        // FAT along all of it.
        let first = stream.first_cluster();
        stream.seek_from(offset, first.and_then(|c| self.fs.dir_chain(c)))?;
        if let (Some(first), Some(at)) = (first, stream.chain_at()) {
            self.fs.set_dir_chain(first, at);
        }
        Ok(DirIter::new(stream, self.fs, true))
    }

    /// The note `set_note` gave this directory, while it holds: until an entry is added to it (a
    /// file or a directory created, an entry moved or renamed into it), it is removed, or
    /// `FileSystem::forget_dir_notes`; and while it is one of the `DIR_NOTES` directories noted
    /// last. Entries removed keep it. None for a root directory outside the data area (FAT12/16).
    #[must_use]
    pub fn note(&self) -> Option<u64> {
        self.fs.dir_note(self.stream.first_cluster()?)
    }

    /// How many entries the file system wrote so far, in any directory (see
    /// `FileSystem::entries_written`).
    #[must_use]
    pub fn entries_written(&self) -> u64 {
        self.fs.entries_written()
    }

    /// Notes `note` about this directory (see `note`), e.g. that it was read whole and how it was.
    pub fn set_note(&self, note: u64) {
        if let Some(cluster) = self.stream.first_cluster() {
            self.fs.set_dir_note(cluster, note);
        }
    }
}

// Modified for PaperPaw: `begins_entry` added.
impl<IO: ReadWriteSeek, TP: TimeProvider, OCC> Dir<'_, IO, TP, OCC> {
    /// Whether an entry begins at byte `offset` of the directory (as `DirEntry::offset_range`
    /// says where one does), a deleted one or the directory's end: not one of the slots inside an
    /// entry of a long name, nor its last one (as where the entry that began there was removed and
    /// another one, of a longer name, written over it). `iter_from` there lists whole entries.
    ///
    /// # Errors
    ///
    /// `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn begins_entry(&self, offset: u64) -> Result<bool, Error<IO::Error>> {
        const LFN: u8 = 0x0F;
        const LFN_FIRST: u8 = 0x40;
        let size = u64::from(DIR_ENTRY_SIZE);
        if offset % size != 0 {
            return Ok(false);
        }
        let mut stream = self.stream.clone();
        let first = stream.first_cluster();
        let start = offset.saturating_sub(size);
        stream.seek_from(start, first.and_then(|c| self.fs.dir_chain(c)))?;
        let mut slots = [[0u8; DIR_ENTRY_SIZE as usize]; 2];
        let at = usize::from(offset > 0);
        for slot in &mut slots[..=at] {
            match stream.read_exact(slot) {
                Ok(()) => {}
                // Past the directory's clusters: its end.
                Err(Error::UnexpectedEof) => return Ok(true),
                Err(e) => return Err(e),
            }
        }
        let live = |slot: &[u8; DIR_ENTRY_SIZE as usize]| {
            slot[0] != 0 && slot[0] != crate::dir_entry::DIR_ENTRY_DELETED_FLAG
        };
        let slot = &slots[at];
        Ok(if !live(slot) {
            true
        } else if slot[11] & LFN == LFN {
            // The first slot of a long name's entries is marked.
            slot[0] & LFN_FIRST != 0
        } else {
            // A short entry after a long name's slots is their last.
            !(at == 1 && live(&slots[0]) && slots[0][11] & LFN == LFN)
        })
    }
}

impl<'a, IO: ReadWriteSeek, TP: TimeProvider, OCC: OemCpConverter> Dir<'a, IO, TP, OCC> {
    fn find_entry(
        &self,
        name: &str,
        is_dir: Option<bool>,
        mut short_name_gen: Option<&mut ShortNameGenerator>,
    ) -> Result<DirEntry<'a, IO, TP, OCC>, Error<IO::Error>> {
        for r in self.iter() {
            let e = r?;
            // compare name ignoring case
            if e.eq_name(name) {
                // check if file or directory is expected
                if is_dir.is_some() && Some(e.is_dir()) != is_dir {
                    if e.is_dir() {
                        error!("Is a directory");
                    } else {
                        error!("Not a directory");
                    }
                    return Err(Error::InvalidInput);
                }
                return Ok(e);
            }
            // update short name generator state
            if let Some(ref mut names) = short_name_gen {
                names.add_existing(e.raw_short_name());
            }
        }
        Err(Error::NotFound) //("No such file or directory"))
    }

    #[allow(clippy::type_complexity)]
    pub(crate) fn find_volume_entry(
        &self,
    ) -> Result<Option<DirEntry<'a, IO, TP, OCC>>, Error<IO::Error>> {
        for r in DirIter::new(self.stream.clone(), self.fs, false) {
            let e = r?;
            if e.data.is_volume() {
                return Ok(Some(e));
            }
        }
        Ok(None)
    }

    fn check_for_existence(
        &self,
        name: &str,
        is_dir: Option<bool>,
    ) -> Result<DirEntryOrShortName<'a, IO, TP, OCC>, Error<IO::Error>> {
        let mut short_name_gen = ShortNameGenerator::new(name);
        loop {
            // find matching entry
            let r = self.find_entry(name, is_dir, Some(&mut short_name_gen));
            match r {
                // file not found - continue with short name generation
                Err(Error::NotFound) => {}
                // unexpected error - return it
                Err(err) => return Err(err),
                // directory already exists - return it
                Ok(e) => return Ok(DirEntryOrShortName::DirEntry(e)),
            }
            // try to generate short name
            if let Ok(name) = short_name_gen.generate() {
                return Ok(DirEntryOrShortName::ShortName(name));
            }
            // there were too many collisions in short name generation
            // try different checksum in the next iteration
            short_name_gen.next_iteration();
        }
    }

    /// Opens existing subdirectory.
    ///
    /// `path` is a '/' separated directory path relative to self directory.
    ///
    /// # Errors
    ///
    /// Errors that can be returned:
    ///
    /// * `Error::NotFound` will be returned if `path` does not point to any existing directory entry.
    /// * `Error::InvalidInput` will be returned if `path` points to a file that is not a directory.
    /// * `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn open_dir(&self, path: &str) -> Result<Self, Error<IO::Error>> {
        trace!("Dir::open_dir {}", path);
        let (name, rest_opt) = split_path(path);
        let e = self.find_entry(name, Some(true), None)?;
        match rest_opt {
            Some(rest) => e.to_dir().open_dir(rest),
            None => Ok(e.to_dir()),
        }
    }

    /// Opens existing file.
    ///
    /// `path` is a '/' separated file path relative to self directory.
    ///
    /// # Errors
    ///
    /// Errors that can be returned:
    ///
    /// * `Error::NotFound` will be returned if `path` points to a non-existing directory entry.
    /// * `Error::InvalidInput` will be returned if `path` points to a file that is a directory.
    /// * `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn open_file(&self, path: &str) -> Result<File<'a, IO, TP, OCC>, Error<IO::Error>> {
        trace!("Dir::open_file {}", path);
        // traverse path
        let (name, rest_opt) = split_path(path);
        if let Some(rest) = rest_opt {
            let e = self.find_entry(name, Some(true), None)?;
            return e.to_dir().open_file(rest);
        }
        // convert entry to a file
        let e = self.find_entry(name, Some(false), None)?;
        Ok(e.to_file())
    }

    /// Creates new or opens existing file=.
    ///
    /// `path` is a '/' separated file path relative to `self` directory.
    /// File is never truncated when opening. It can be achieved by calling `File::truncate` method after opening.
    ///
    /// # Errors
    ///
    /// Errors that can be returned:
    ///
    /// * `Error::InvalidInput` will be returned if `path` points to an existing file that is a directory.
    /// * `Error::InvalidFileNameLength` will be returned if the file name is empty or if it is too long.
    /// * `Error::UnsupportedFileNameCharacter` will be returned if the file name contains an invalid character.
    /// * `Error::NotEnoughSpace` will be returned if there is not enough free space to create a new file.
    /// * `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn create_file(&self, path: &str) -> Result<File<'a, IO, TP, OCC>, Error<IO::Error>> {
        trace!("Dir::create_file {}", path);
        // traverse path
        let (name, rest_opt) = split_path(path);
        if let Some(rest) = rest_opt {
            return self
                .find_entry(name, Some(true), None)?
                .to_dir()
                .create_file(rest);
        }
        // this is final filename in the path
        let r = self.check_for_existence(name, Some(false))?;
        match r {
            // file does not exist - create it
            DirEntryOrShortName::ShortName(short_name) => {
                let sfn_entry =
                    self.create_sfn_entry(short_name, FileAttributes::from_bits_truncate(0), None);
                Ok(self.write_entry(name, sfn_entry)?.to_file())
            }
            // file already exists - return it
            DirEntryOrShortName::DirEntry(e) => Ok(e.to_file()),
        }
    }

    // Modified for PaperPaw: added.
    /// As [`create_file`](Self::create_file), for a file `name` of this directory that is about
    /// to be written: a new file takes its first cluster here, and its entry is written once,
    /// with that cluster in it. Said on the disk first (see `fs::claim`): cut off before the
    /// entry is written (power lost), the cluster is freed by the next mount. `create_file`
    /// writes the entry, and writes it again when the first write takes a cluster.
    ///
    /// # Errors
    ///
    /// As `create_file`.
    pub fn create_file_with_cluster(
        &self,
        name: &str,
    ) -> Result<File<'a, IO, TP, OCC>, Error<IO::Error>> {
        trace!("Dir::create_file_with_cluster {}", name);
        match self.check_for_existence(name, Some(false))? {
            DirEntryOrShortName::ShortName(short_name) => {
                // This directory grows first where the entry needs it: a change with a word of
                // its own.
                let referrer = self
                    .make_room(name)?
                    .map_or(Referrer::Unknown, Referrer::NewEntry);
                let (cluster, _) = self.fs.alloc_clusters(None, 1, referrer)?;
                let sfn_entry = self.create_sfn_entry(
                    short_name,
                    FileAttributes::from_bits_truncate(0),
                    Some(cluster),
                );
                match self.write_entry(name, sfn_entry) {
                    Ok(entry) => {
                        self.written_where_said(&entry, referrer)?;
                        Ok(entry.to_file())
                    }
                    Err(err) => {
                        if let Err(err) = self.fs.free_cluster_chain(cluster) {
                            error!("the new file's cluster stays taken {:?}", err);
                        }
                        Err(err)
                    }
                }
            }
            DirEntryOrShortName::DirEntry(e) => Ok(e.to_file()),
        }
    }

    /// Creates new directory or opens existing.
    ///
    /// `path` is a '/' separated path relative to self directory.
    ///
    /// # Errors
    ///
    /// Errors that can be returned:
    ///
    /// * `Error::InvalidInput` will be returned if `path` points to an existing file that is not a directory.
    /// * `Error::InvalidFileNameLength` will be returned if the file name is empty or if it is too long.
    /// * `Error::UnsupportedFileNameCharacter` will be returned if the file name contains an invalid character.
    /// * `Error::NotEnoughSpace` will be returned if there is not enough free space to create a new directory.
    /// * `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn create_dir(&self, path: &str) -> Result<Self, Error<IO::Error>> {
        trace!("Dir::create_dir {}", path);
        // traverse path
        let (name, rest_opt) = split_path(path);
        if let Some(rest) = rest_opt {
            return self
                .find_entry(name, Some(true), None)?
                .to_dir()
                .create_dir(rest);
        }
        // this is final filename in the path
        let r = self.check_for_existence(name, Some(true))?;
        match r {
            // directory does not exist - create it
            DirEntryOrShortName::ShortName(short_name) => {
                // Modified for PaperPaw: the new directory's cluster is said on the disk before
                // it is taken (see `fs::claim`): cut off before its entry is written, it is
                // freed by the next mount. This directory grows first where the entry needs it,
                // a change with a word of its own; the cluster is given back where the entry
                // cannot be written.
                let referrer = self
                    .make_room(name)?
                    .map_or(Referrer::Unknown, Referrer::NewEntry);
                // alloc cluster for directory data
                let cluster = self.fs.alloc_cluster(None, referrer)?;
                // create entry in parent directory
                let sfn_entry =
                    self.create_sfn_entry(short_name, FileAttributes::DIRECTORY, Some(cluster));
                let entry = match self.write_entry(name, sfn_entry) {
                    Ok(entry) => entry,
                    Err(err) => {
                        if let Err(err) = self.fs.free_cluster_chain(cluster) {
                            error!("the new directory's cluster stays taken {:?}", err);
                        }
                        return Err(err);
                    }
                };
                self.written_where_said(&entry, referrer)?;
                let dir = entry.to_dir();
                // create special entries "." and ".."
                let dot_sfn = ShortNameGenerator::generate_dot();
                let sfn_entry = self.create_sfn_entry(
                    dot_sfn,
                    FileAttributes::DIRECTORY,
                    entry.first_cluster(),
                );
                dir.write_entry(".", sfn_entry)?;
                let dotdot_sfn = ShortNameGenerator::generate_dotdot();
                // cluster of the root dir shall be set to 0 in directory entries.
                let dotdot_cluster = if self.stream.is_root_dir() {
                    None
                } else {
                    self.stream.first_cluster()
                };
                let sfn_entry =
                    self.create_sfn_entry(dotdot_sfn, FileAttributes::DIRECTORY, dotdot_cluster);
                dir.write_entry("..", sfn_entry)?;
                Ok(dir)
            }
            // directory already exists - return it
            DirEntryOrShortName::DirEntry(e) => Ok(e.to_dir()),
        }
    }

    fn is_empty(&self) -> Result<bool, Error<IO::Error>> {
        trace!("Dir::is_empty");
        // check if directory contains no files
        for r in self.iter() {
            let e = r?;
            let name = e.short_file_name_as_bytes();
            // ignore special entries "." and ".."
            if name != b"." && name != b".." {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Removes existing file or directory.
    ///
    /// `path` is a '/' separated file path relative to self directory.
    /// Make sure there is no reference to this file (no File instance) or filesystem corruption
    /// can happen.
    ///
    /// # Errors
    ///
    /// Errors that can be returned:
    ///
    /// * `Error::NotFound` will be returned if `path` points to a non-existing directory entry.
    /// * `Error::InvalidInput` will be returned if `path` points to a file that is not a directory.
    /// * `Error::DirectoryIsNotEmpty` will be returned if the specified directory is not empty.
    /// * `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn remove(&self, path: &str) -> Result<(), Error<IO::Error>> {
        trace!("Dir::remove {}", path);
        // traverse path
        let (name, rest_opt) = split_path(path);
        if let Some(rest) = rest_opt {
            let e = self.find_entry(name, Some(true), None)?;
            return e.to_dir().remove(rest);
        }
        // in case of directory check if it is empty
        let e = self.find_entry(name, None, None)?;
        self.remove_found(&e)
    }

    // Modified for PaperPaw: `remove_found` added, `remove` through it.

    /// Removes entry `e` of this directory, found by `iter` (or `iter_from`) and nothing changed
    /// in the directory since: a file, or an empty directory. Make sure there is no reference to
    /// it (no File instance) or filesystem corruption can happen.
    ///
    /// # Errors
    ///
    /// * `Error::DirectoryIsNotEmpty` will be returned if `e` is a directory that is not empty.
    /// * `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn remove_found(&self, e: &DirEntry<'a, IO, TP, OCC>) -> Result<(), Error<IO::Error>> {
        if e.is_dir() && !e.to_dir().is_empty()? {
            return Err(Error::DirectoryIsNotEmpty);
        }
        // Modified for PaperPaw: the entries are removed before the data is
        // freed. Cut off between (power lost), the clusters are taken by
        // no file, instead of a file pointing at free clusters that may be
        // given to another; the next mount frees them (see `fs::claim`).
        self.remove_and_free(e)
    }

    // Modified for PaperPaw: added.
    /// The entries of `e` removed, then its data freed: said on the disk first, so that a cut
    /// between leaves no cluster taken by no file (see `fs::claim`).
    fn remove_and_free(&self, e: &DirEntry<'a, IO, TP, OCC>) -> Result<(), Error<IO::Error>> {
        let first = e.first_cluster();
        // An entry whose first cluster is none of the volume's (damaged) is removed all the
        // same; freeing its data fails then (`free_claimed`), nothing of the FAT written.
        let claimed = first.filter(|&n| self.fs.is_cluster(n));
        if let Some(n) = claimed {
            self.fs.claim_freeing(n, Referrer::Entry(e.entry_pos))?;
        }
        if let Err(err) = self.remove_entries(e) {
            // The entry may still be there: nothing says its data is no file's.
            if claimed.is_some() && self.fs.drop_claim().is_err() {
                error!("a claim stays on the disk");
            }
            return Err(err);
        }
        if let Some(n) = first {
            // Its clusters may hold another directory once freed.
            if e.is_dir() {
                self.fs.drop_dir_note(n);
            }
            // free data
            self.fs.free_claimed(n, claimed.is_some())?;
        }
        Ok(())
    }

    // Modified for PaperPaw: added.

    /// [`remove_found`](Self::remove_found) for a directory the caller knows to be empty (it
    /// removed every entry in it itself): not read through again, which for a directory that had
    /// thousands of entries is all of its clusters. Entries still in it are lost with it, their
    /// data's clusters left taken.
    ///
    /// # Errors
    ///
    /// * `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn remove_found_emptied(
        &self,
        e: &DirEntry<'a, IO, TP, OCC>,
    ) -> Result<(), Error<IO::Error>> {
        self.remove_and_free(e)
    }

    // Modified for PaperPaw: `remove_entries`, `unlink`, `replace` and `free_short_name` added.

    /// Marks the long and short name entries of `e` deleted, leaving its data.
    fn remove_entries(&self, e: &DirEntry<'a, IO, TP, OCC>) -> Result<(), Error<IO::Error>> {
        let mut stream = self.stream.clone();
        stream.seek(SeekFrom::Start(e.offset_range.0))?;
        let num = ((e.offset_range.1 - e.offset_range.0) / u64::from(DIR_ENTRY_SIZE)) as usize;
        for _ in 0..num {
            let mut data = DirEntryData::deserialize(&mut stream)?;
            trace!("removing dir entry {:?}", data);
            data.set_deleted();
            stream.seek(SeekFrom::Current(-i64::from(DIR_ENTRY_SIZE)))?;
            data.serialize(&mut stream)?;
            // Modified for PaperPaw: not to be taken for a place no entry was written to (see
            // `fs::claim`).
            if let Some(end) = stream.abs_pos() {
                self.fs.unlike_claim(end - u64::from(DIR_ENTRY_SIZE))?;
            }
        }
        Ok(())
    }

    /// Removes the entries of `name` (a file or a directory), leaving its data: for an entry whose
    /// data another entry holds too, as a `rename` cut off between leaves it.
    /// Make sure there is no reference to it (no File instance) or filesystem corruption can happen.
    ///
    /// # Errors
    ///
    /// Errors that can be returned:
    ///
    /// * `Error::NotFound` will be returned if `name` names no entry.
    /// * `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn unlink(&self, name: &str) -> Result<(), Error<IO::Error>> {
        trace!("Dir::unlink {}", name);
        let e = self.find_entry(name, None, None)?;
        self.fs.drop_free_claim_of(e.first_cluster())?;
        self.remove_entries(&e)
    }

    // Modified for PaperPaw: added.

    /// Removes the entries of `e`, found by `iter` (or `iter_from`) and nothing changed in the
    /// directory since, leaving its data: for an entry that cannot be named (one of a damaged
    /// directory), whose data may be another's. A directory's entries are left as they are.
    /// Make sure there is no reference to it (no File instance) or filesystem corruption can
    /// happen.
    ///
    /// # Errors
    ///
    /// * `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn unlink_found(&self, e: &DirEntry<'a, IO, TP, OCC>) -> Result<(), Error<IO::Error>> {
        if let Some(n) = e.first_cluster().filter(|_| e.is_dir()) {
            self.fs.drop_dir_note(n);
        }
        self.fs.drop_free_claim_of(e.first_cluster())?;
        self.remove_entries(e)
    }

    /// Puts file `src_name` in the place of file `dst_name`, both in this directory, removing the
    /// file that was there.
    ///
    /// The new entry of `dst_name` is written before the old one and the entry of `src_name` are
    /// removed: cut off at any point (power lost), `dst_name` is the old file or the new one,
    /// never missing; the old file and the new one may both be named `dst_name` for a moment
    /// (the new one holding the data of `src_name`, still there), or both names be the new file.
    /// Called again then, `replace` finishes the work. The old data is freed last. An empty new
    /// file holds no data that tells its new entry: an empty one named `dst_name` is taken for it
    /// (the same content). The new entry has the creation time of the old file, where there is one;
    /// else that of `src_name`, but where it holds a mark (see `File::set_mark`): the time now.
    /// Make sure there is no reference to either file (no File instance) or filesystem corruption
    /// can happen.
    ///
    /// # Errors
    ///
    /// Errors that can be returned:
    ///
    /// * `Error::NotFound` will be returned if `src_name` names no entry.
    /// * `Error::InvalidInput` will be returned if either name is a directory.
    /// * `Error::InvalidFileNameLength` or `Error::UnsupportedFileNameCharacter` will be returned if
    ///   `dst_name` cannot be a long name.
    /// * `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn replace(&self, src_name: &str, dst_name: &str) -> Result<(), Error<IO::Error>> {
        trace!("Dir::replace {} {}", src_name, dst_name);
        // Refused before anything is changed.
        validate_long_name(dst_name)?;
        let src = self.find_entry(src_name, Some(false), None)?;
        // The old file, and whether the new one is there already (an earlier replace cut off
        // after it wrote the new entry): both may be named `dst_name`.
        let mut old = None;
        let mut new_there = false;
        for r in self.iter() {
            let e = r?;
            if !e.eq_name(dst_name) {
                continue;
            }
            if e.is_same_entry(&src) {
                return Ok(());
            }
            if e.is_dir() {
                return Err(Error::InvalidInput);
            }
            let holds_src = match src.first_cluster() {
                Some(_) => e.first_cluster() == src.first_cluster(),
                // Once: a second one is the old file, empty too.
                None => !new_there && e.first_cluster().is_none() && e.len() == 0,
            };
            if holds_src {
                new_there = true;
            } else if old.is_none() {
                old = Some(e);
            } else {
                return Err(Error::AlreadyExists);
            }
        }
        if !new_there {
            let short_name = self.free_short_name(dst_name)?;
            let mut data = src.data.renamed(short_name);
            match &old {
                Some(old) => data = data.with_created_of(&old.data),
                // A mark the caller gave `src_name` is not left to the file.
                None if data.mark().is_some() => {
                    data.set_created(self.fs.options.time_provider.get_current_date_time());
                }
                None => {}
            }
            self.write_entry(dst_name, data)?;
        }
        // As `remove`: the entries first, then the data, said on the disk before.
        self.fs.drop_free_claim_of(src.first_cluster())?;
        let old_data = old.as_ref().and_then(DirEntry::first_cluster);
        let claimed = old_data.filter(|&n| self.fs.is_cluster(n));
        if let Some(old) = &old {
            if let Some(n) = claimed {
                self.fs.claim_freeing(n, Referrer::Entry(old.entry_pos))?;
            }
            if let Err(err) = self.remove_entries(old) {
                if claimed.is_some() && self.fs.drop_claim().is_err() {
                    error!("a claim stays on the disk");
                }
                return Err(err);
            }
        }
        self.remove_entries(&src)?;
        if let Some(n) = old_data {
            self.fs.free_claimed(n, claimed.is_some())?;
        }
        Ok(())
    }

    /// A short name for a new entry `name` that no entry has, whether or not an entry is named
    /// `name`.
    fn free_short_name(&self, name: &str) -> Result<[u8; SFN_SIZE], Error<IO::Error>> {
        let mut short_name_gen = ShortNameGenerator::new(name);
        loop {
            for r in self.iter() {
                short_name_gen.add_existing(r?.raw_short_name());
            }
            if let Ok(short_name) = short_name_gen.generate() {
                return Ok(short_name);
            }
            short_name_gen.next_iteration();
        }
    }

    /// Renames or moves existing file or directory.
    ///
    /// `src_path` is a '/' separated source file path relative to self directory.
    /// `dst_path` is a '/' separated destination file path relative to `dst_dir`.
    /// `dst_dir` can be set to self directory if rename operation without moving is needed.
    /// Make sure there is no reference to this file (no File instance) or filesystem corruption
    /// can happen.
    ///
    /// # Errors
    ///
    /// Errors that can be returned:
    ///
    /// * `Error::NotFound` will be returned if `src_path` points to a non-existing directory entry or if `dst_path`
    ///   stripped from the last component does not point to an existing directory.
    /// * `Error::AlreadyExists` will be returned if `dst_path` points to an existing directory entry.
    /// * `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn rename(
        &self,
        src_path: &str,
        dst_dir: &Dir<IO, TP, OCC>,
        dst_path: &str,
    ) -> Result<(), Error<IO::Error>> {
        trace!("Dir::rename {} {}", src_path, dst_path);
        // traverse source path
        let (src_name, src_rest_opt) = split_path(src_path);
        if let Some(rest) = src_rest_opt {
            let e = self.find_entry(src_name, Some(true), None)?;
            return e.to_dir().rename(rest, dst_dir, dst_path);
        }
        // traverse destination path
        let (dst_name, dst_rest_opt) = split_path(dst_path);
        if let Some(rest) = dst_rest_opt {
            let e = dst_dir.find_entry(dst_name, Some(true), None)?;
            return self.rename(src_path, &e.to_dir(), rest);
        }
        // move/rename file
        self.rename_internal(src_path, dst_dir, dst_path)
    }

    fn rename_internal(
        &self,
        src_name: &str,
        dst_dir: &Dir<IO, TP, OCC>,
        dst_name: &str,
    ) -> Result<(), Error<IO::Error>> {
        trace!("Dir::rename_internal {} {}", src_name, dst_name);
        // find existing file
        let e = self.find_entry(src_name, None, None)?;
        self.rename_found(&e, dst_dir, dst_name)
    }

    // Modified for PaperPaw: `rename_found` added, `rename_internal` through it.

    /// Moves entry `e` of this directory, found by `iter` (or `iter_from`) and nothing changed in
    /// the directory since, to `dst_name` in `dst_dir` (a name, not a path), as `rename` does.
    /// Make sure there is no reference to this file (no File instance) or filesystem corruption
    /// can happen.
    ///
    /// # Errors
    ///
    /// * `Error::AlreadyExists` will be returned if `dst_name` names another entry of `dst_dir`.
    /// * `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn rename_found(
        &self,
        e: &DirEntry<'a, IO, TP, OCC>,
        dst_dir: &Dir<IO, TP, OCC>,
        dst_name: &str,
    ) -> Result<(), Error<IO::Error>> {
        trace!("Dir::rename_found {}", dst_name);
        // check if destionation filename is unused
        let r = dst_dir.check_for_existence(dst_name, None)?;
        let short_name = match r {
            // destination file already exist
            DirEntryOrShortName::DirEntry(ref dst_e) => {
                // check if source and destination entry is the same
                if e.is_same_entry(dst_e) {
                    // nothing to do
                    return Ok(());
                }
                // destination file exists and it is not the same as source file - fail
                return Err(Error::AlreadyExists);
            }
            // destionation file does not exist, short name has been generated
            DirEntryOrShortName::ShortName(short_name) => short_name,
        };
        // Modified for PaperPaw: the new entry is written before the old one is removed, as in
        // `replace`. Cut off between (power lost), the entry is at both places, pointing at the same
        // data, never at neither; the caller settles which one stays (see `unlink`).
        let sfn_entry = e.data.renamed(short_name);
        dst_dir.write_entry(dst_name, sfn_entry)?;
        self.fs.drop_free_claim_of(e.first_cluster())?;
        self.remove_entries(e)?;
        if e.is_dir() {
            e.to_dir().point_up(dst_dir)?;
        }
        Ok(())
    }

    // Modified for PaperPaw: added.

    /// Points the `..` entry of this directory at `parent`, the directory it is in, where it
    /// points elsewhere: the last step of moving a directory (`rename_found`), which a move cut
    /// off (power lost) may not have made.
    ///
    /// # Errors
    ///
    /// * `Error::NotFound` will be returned if this directory has no `..` entry (the root).
    /// * `Error::Io` will be returned if the underlying storage object returned an I/O error.
    pub fn point_up(&self, parent: &Dir<IO, TP, OCC>) -> Result<(), Error<IO::Error>> {
        let parent_cluster = if parent.stream.is_root_dir() {
            None
        } else {
            parent.stream.first_cluster()
        };
        let dotdot = self.find_entry("..", Some(true), None)?;
        if dotdot.first_cluster() != parent_cluster {
            dotdot.set_first_cluster(parent_cluster)?;
        }
        Ok(())
    }

    // Modified for PaperPaw: added.
    /// After `entry` was written with the cluster a claim was made for (`referrer`: where
    /// `make_room` said the entry would be): were it written elsewhere, the claim would take
    /// the cluster for one that never got its entry, so it is taken away.
    fn written_where_said(
        &self,
        entry: &DirEntry<'a, IO, TP, OCC>,
        referrer: Referrer,
    ) -> Result<(), Error<IO::Error>> {
        let as_said = referrer == Referrer::NewEntry(entry.entry_pos);
        debug_assert!(
            as_said || referrer == Referrer::Unknown,
            "an entry written elsewhere than said"
        );
        if as_said {
            Ok(())
        } else {
            self.fs.drop_claim()
        }
    }

    // Modified for PaperPaw: added.
    /// Grows this directory now where an entry named `name` would not fit in its clusters: the
    /// entry is then written without taking a cluster. Where on the disk its short name entry
    /// will be written (by `write_entry`, nothing changed in the directory between). Nothing is
    /// done for the root directory of FAT12 and FAT16, which does not grow.
    fn make_room(&self, name: &str) -> Result<Option<u64>, Error<IO::Error>> {
        if matches!(self.stream, DirRawStream::Root(_)) {
            return Ok(None);
        }
        validate_long_name(name)?;
        let lfn_utf16 = Self::encode_lfn_utf16(name);
        let entries = LfnEntriesGenerator::new(lfn_utf16.as_ucs2_units(), 0).len() as u64 + 1;
        let mut stream = self.find_free_entries(entries as u32)?;
        let size = u64::from(DIR_ENTRY_SIZE);
        let end = stream.seek(SeekFrom::Current(0))? + entries * size;
        // An entry of zeros ends a directory: one written where its clusters end takes the next.
        loop {
            let reached = stream.seek(SeekFrom::Start(end))?;
            if reached >= end {
                break;
            }
            stream.write_all(&[0_u8; DIR_ENTRY_SIZE as usize])?;
        }
        // Read, the place of the last entry is the one the stream stands after.
        stream.seek(SeekFrom::Start(end - size))?;
        let mut slot = [0_u8; DIR_ENTRY_SIZE as usize];
        stream.read_exact(&mut slot)?;
        Ok(stream.abs_pos().map(|after| after - size))
    }

    fn find_free_entries(
        &self,
        num_entries: u32,
    ) -> Result<DirRawStream<'a, IO, TP, OCC>, Error<IO::Error>> {
        let mut stream = self.stream.clone();
        let mut first_free: u32 = 0;
        let mut num_free: u32 = 0;
        let mut i: u32 = 0;
        loop {
            let raw_entry = DirEntryData::deserialize(&mut stream)?;
            if raw_entry.is_end() {
                // first unused entry - all remaining space can be used
                if num_free == 0 {
                    first_free = i;
                }
                let pos = u64::from(first_free * DIR_ENTRY_SIZE);
                stream.seek(io::SeekFrom::Start(pos))?;
                return Ok(stream);
            } else if raw_entry.is_deleted() {
                // free entry - calculate number of free entries in a row
                if num_free == 0 {
                    first_free = i;
                }
                num_free += 1;
                if num_free == num_entries {
                    // enough space for new file
                    let pos = u64::from(first_free * DIR_ENTRY_SIZE);
                    stream.seek(io::SeekFrom::Start(pos))?;
                    return Ok(stream);
                }
            } else {
                // used entry - start counting from 0
                num_free = 0;
            }
            i += 1;
        }
    }

    fn create_sfn_entry(
        &self,
        short_name: [u8; SFN_SIZE],
        attrs: FileAttributes,
        first_cluster: Option<u32>,
    ) -> DirFileEntryData {
        let mut raw_entry = DirFileEntryData::new(short_name, attrs);
        raw_entry.set_first_cluster(first_cluster, self.fs.fat_type());
        let now = self.fs.options.time_provider.get_current_date_time();
        raw_entry.set_created(now);
        raw_entry.set_accessed(now.date);
        raw_entry.set_modified(now);
        raw_entry
    }

    fn encode_lfn_utf16(name: &str) -> LfnBuffer {
        LfnBuffer::from_ucs2_units(name.encode_utf16())
    }

    #[allow(clippy::type_complexity)]
    fn alloc_and_write_lfn_entries(
        &self,
        lfn_utf16: &LfnBuffer,
        short_name: &[u8; SFN_SIZE],
    ) -> Result<(DirRawStream<'a, IO, TP, OCC>, u64), Error<IO::Error>> {
        // get short name checksum
        let lfn_chsum = lfn_checksum(short_name);
        // create LFN entries generator
        let lfn_iter = LfnEntriesGenerator::new(lfn_utf16.as_ucs2_units(), lfn_chsum);
        // find space for new entries (multiple LFN entries and 1 SFN entry)
        let num_entries = lfn_iter.len() as u32 + 1;
        let mut stream = self.find_free_entries(num_entries)?;
        let start_pos = stream.seek(io::SeekFrom::Current(0))?;
        // write LFN entries before SFN entry
        for lfn_entry in lfn_iter {
            lfn_entry.serialize(&mut stream)?;
        }
        Ok((stream, start_pos))
    }

    fn alloc_sfn_entry(&self) -> Result<(DirRawStream<'a, IO, TP, OCC>, u64), Error<IO::Error>> {
        let mut stream = self.find_free_entries(1)?;
        let start_pos = stream.seek(io::SeekFrom::Current(0))?;
        Ok((stream, start_pos))
    }

    fn write_entry(
        &self,
        name: &str,
        raw_entry: DirFileEntryData,
    ) -> Result<DirEntry<'a, IO, TP, OCC>, Error<IO::Error>> {
        trace!("Dir::write_entry {}", name);
        // check if name doesn't contain unsupported characters
        validate_long_name(name)?;
        // Modified for PaperPaw: an entry that holds no cluster may be, byte for byte, the one a
        // claim of a new file's first cluster names (see `fs::claim`): that claim goes first.
        if raw_entry.first_cluster(self.fs.fat_type()).is_none() {
            self.fs.drop_entry_claim()?;
        }
        // Modified for PaperPaw: counted, and a note about the directory holds no longer (see
        // `note`).
        self.fs.note_entry_written(self.stream.first_cluster());
        // convert long name to UTF-16
        let lfn_utf16 = Self::encode_lfn_utf16(name);
        // write LFN entries, except for . and .., which need to be at
        // the first two slots and don't need LFNs anyway
        let (mut stream, start_pos) = if name == "." || name == ".." {
            self.alloc_sfn_entry()?
        } else {
            self.alloc_and_write_lfn_entries(&lfn_utf16, raw_entry.name())?
        };
        // write short name entry
        raw_entry.serialize(&mut stream)?;
        // Get position directory stream after entries were written
        let end_pos = stream.seek(io::SeekFrom::Current(0))?;
        // Get current absolute position on the storage
        // Unwrapping is safe because abs_pos() returns None only if stream is at position 0. This is not
        // the case because an entry was just written
        // Note: if current position is on the cluster boundary then a position in the cluster containing the entry is
        // returned
        let end_abs_pos = stream.abs_pos().unwrap();
        // Calculate SFN entry start position on the storage
        let start_abs_pos = end_abs_pos - u64::from(DIR_ENTRY_SIZE);
        // return new logical entry descriptor
        let short_name = ShortName::new(raw_entry.name());
        Ok(DirEntry {
            data: raw_entry,
            short_name,
            lfn_utf16,
            fs: self.fs,
            entry_pos: start_abs_pos,
            offset_range: (start_pos, end_pos),
        })
    }
}

// Note: derive cannot be used because of invalid bounds. See: https://github.com/rust-lang/rust/issues/26925
impl<IO: ReadWriteSeek, TP: TimeProvider, OCC: OemCpConverter> Clone for Dir<'_, IO, TP, OCC> {
    fn clone(&self) -> Self {
        Self {
            stream: self.stream.clone(),
            fs: self.fs,
        }
    }
}

/// An iterator over the directory entries.
///
/// This struct is created by the `iter` method on `Dir`.
pub struct DirIter<'a, IO: ReadWriteSeek, TP, OCC> {
    stream: DirRawStream<'a, IO, TP, OCC>,
    fs: &'a FileSystem<IO, TP, OCC>,
    skip_volume: bool,
    err: bool,
}

impl<'a, IO: ReadWriteSeek, TP, OCC> DirIter<'a, IO, TP, OCC> {
    fn new(
        stream: DirRawStream<'a, IO, TP, OCC>,
        fs: &'a FileSystem<IO, TP, OCC>,
        skip_volume: bool,
    ) -> Self {
        DirIter {
            stream,
            fs,
            skip_volume,
            err: false,
        }
    }
}

impl<'a, IO: ReadWriteSeek, TP: TimeProvider, OCC> DirIter<'a, IO, TP, OCC> {
    fn should_skip_entry(&self, raw_entry: &DirEntryData) -> bool {
        if raw_entry.is_deleted() {
            return true;
        }
        match raw_entry {
            DirEntryData::File(sfn_entry) => self.skip_volume && sfn_entry.is_volume(),
            DirEntryData::Lfn(_) => false,
        }
    }

    #[allow(clippy::type_complexity)]
    fn read_dir_entry(&mut self) -> Result<Option<DirEntry<'a, IO, TP, OCC>>, Error<IO::Error>> {
        trace!("DirIter::read_dir_entry");
        let mut lfn_builder = LongNameBuilder::new();
        let mut offset = self.stream.seek(SeekFrom::Current(0))?;
        let mut begin_offset = offset;
        loop {
            let raw_entry = DirEntryData::deserialize(&mut self.stream)?;
            offset += u64::from(DIR_ENTRY_SIZE);
            // Check if this is end of dir
            if raw_entry.is_end() {
                return Ok(None);
            }
            // Check if this is deleted or volume ID entry
            if self.should_skip_entry(&raw_entry) {
                trace!("skip entry");
                lfn_builder.clear();
                begin_offset = offset;
                continue;
            }
            match raw_entry {
                DirEntryData::File(data) => {
                    // Get current absolute position on the storage
                    // Unwrapping is safe because abs_pos() returns None only if stream is at position 0. This is not
                    // the case because an entry was just read
                    // Note: if current position is on the cluster boundary then a position in the cluster containing the entry is
                    // returned
                    let end_abs_pos = self.stream.abs_pos().unwrap();
                    // Calculate SFN entry start position on the storage
                    let abs_pos = end_abs_pos - u64::from(DIR_ENTRY_SIZE);
                    // Check if LFN checksum is valid
                    lfn_builder.validate_chksum(data.name());
                    // Return directory entry
                    let short_name = ShortName::new(data.name());
                    trace!("file entry {:?}", data.name());
                    return Ok(Some(DirEntry {
                        data,
                        short_name,
                        lfn_utf16: lfn_builder.into_buf(),
                        fs: self.fs,
                        entry_pos: abs_pos,
                        offset_range: (begin_offset, offset),
                    }));
                }
                DirEntryData::Lfn(data) => {
                    // Append to LFN buffer
                    trace!("lfn entry");
                    lfn_builder.process(&data);
                }
            }
        }
    }
}

// Note: derive cannot be used because of invalid bounds. See: https://github.com/rust-lang/rust/issues/26925
impl<IO: ReadWriteSeek, TP, OCC> Clone for DirIter<'_, IO, TP, OCC> {
    fn clone(&self) -> Self {
        Self {
            stream: self.stream.clone(),
            fs: self.fs,
            err: self.err,
            skip_volume: self.skip_volume,
        }
    }
}

impl<'a, IO: ReadWriteSeek, TP: TimeProvider, OCC> Iterator for DirIter<'a, IO, TP, OCC> {
    type Item = Result<DirEntry<'a, IO, TP, OCC>, Error<IO::Error>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.err {
            return None;
        }
        let r = self.read_dir_entry();
        match r {
            Ok(Some(e)) => Some(Ok(e)),
            Ok(None) => None,
            Err(err) => {
                self.err = true;
                Some(Err(err))
            }
        }
    }
}

#[rustfmt::skip]
// Modified for PaperPaw: added.
/// Whether `name` can be the long name of a file or a directory, as creating one checks it.
pub fn is_valid_long_name(name: &str) -> bool {
    validate_long_name::<()>(name).is_ok()
}

pub(crate) fn validate_long_name<E: IoError>(name: &str) -> Result<(), Error<E>> {
    // check if length is valid
    if name.is_empty() {
        return Err(Error::InvalidFileNameLength);
    }
    if name.len() > MAX_LONG_NAME_LEN {
        return Err(Error::InvalidFileNameLength);
    }
    // check if there are only valid characters
    for c in name.chars() {
        match c {
            'a'..='z'
            | 'A'..='Z'
            | '0'..='9'
            | '\u{80}'..='\u{FFFF}'
            | '$'
            | '%'
            | '\''
            | '-'
            | '_'
            | '@'
            | '~'
            | '`'
            | '!'
            | '('
            | ')'
            | '{'
            | '}'
            | '.'
            | ' '
            | '+'
            | ','
            | ';'
            | '='
            | '['
            | ']'
            | '^'
            | '#'
            | '&' => {}
            _ => return Err(Error::UnsupportedFileNameCharacter),
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "dir_tests.rs"]
mod tests;
