//! A file of an exFAT volume. Its clusters are marked taken before its
//! entry set names them, and its entry set (size, first cluster) is written
//! by [`XFile::flush`] in one block's write: cut off before, the file is as
//! it was and the new clusters are taken by no file.

use super::dir::{self, Loc};
use super::layout::FAT_END;
use super::{Chain, ExFat, Res};
use crate::dir_entry::FileAttributes;
use crate::error::Error;
use crate::fs::ReadWriteSeek;
use crate::io::SeekFrom;

pub struct XFile<'a, IO: ReadWriteSeek> {
    fs: &'a ExFat<IO>,
    chain: Chain,
    /// Its size.
    len: u64,
    /// The bytes written; from there to its size it reads as zeros.
    valid: u64,
    pos: u64,
    /// An index into its clusters and the cluster there.
    walked: (u32, u32),
    loc: Option<Loc>,
    /// Its entry set is to be written.
    dirty: bool,
    mark: Option<u32>,
    attrs: Option<FileAttributes>,
    modified: Option<u32>,
}

impl<IO: ReadWriteSeek> Clone for XFile<'_, IO> {
    fn clone(&self) -> Self {
        Self {
            fs: self.fs,
            chain: self.chain,
            len: self.len,
            valid: self.valid,
            pos: self.pos,
            walked: self.walked,
            loc: self.loc,
            dirty: self.dirty,
            mark: self.mark,
            attrs: self.attrs,
            modified: self.modified,
        }
    }
}

impl<'a, IO: ReadWriteSeek> XFile<'a, IO> {
    pub(crate) fn new(
        fs: &'a ExFat<IO>,
        chain: Chain,
        len: u64,
        valid: u64,
        loc: Option<Loc>,
    ) -> Self {
        Self {
            fs,
            chain,
            len,
            valid: valid.min(len),
            pos: 0,
            walked: (0, chain.first),
            loc,
            dirty: false,
            mark: None,
            attrs: None,
            modified: None,
        }
    }

    /// The clusters its size says it holds.
    fn clusters(&self) -> u32 {
        u32::try_from(self.len.div_ceil(self.fs.cluster_size())).unwrap_or(u32::MAX)
    }

    fn cluster(&mut self, index: u32) -> Res<u32, IO> {
        self.fs
            .cluster_at(self.chain, &mut self.walked, index)?
            .ok_or(Error::CorruptedFileSystem)
    }

    pub(crate) fn set_mark(&mut self, mark: u32) {
        self.mark = Some(mark);
        self.dirty = true;
    }

    pub(crate) fn set_modified(&mut self, at: crate::time::DateTime) {
        let (time, _) = at.time.encode();
        self.modified = Some((u32::from(at.date.encode()) << 16) | u32::from(time));
        self.dirty = true;
    }

    pub(crate) fn set_attributes(&mut self, attrs: FileAttributes) {
        self.attrs = Some(attrs);
        self.dirty = true;
    }

    pub(crate) fn read(&mut self, buf: &mut [u8]) -> Res<usize, IO> {
        let left = self.len.saturating_sub(self.pos);
        let mut n = buf.len().min(usize::try_from(left).unwrap_or(usize::MAX));
        if n == 0 {
            return Ok(0);
        }
        if self.pos >= self.valid {
            buf[..n].fill(0);
            self.pos += n as u64;
            return Ok(n);
        }
        n = n.min(usize::try_from(self.valid - self.pos).unwrap_or(usize::MAX));
        let size = self.fs.cluster_size();
        let within = self.pos & (size - 1);
        if !self.chain.contiguous {
            // To the end of its cluster: the next may lie anywhere.
            n = n.min((size - within) as usize);
        }
        let cluster = self.cluster((self.pos >> self.fs.cluster_shift()) as u32)?;
        // A run past the volume's end fails the read, as it must.
        let at = self.fs.cluster_offset(cluster)? + within;
        self.fs.read_at(at, &mut buf[..n])?;
        self.pos += n as u64;
        Ok(n)
    }

    /// The cluster the next byte is written to, taken where the file has
    /// none there yet.
    fn cluster_to_write(&mut self) -> Res<u32, IO> {
        let index = (self.pos >> self.fs.cluster_shift()) as u32;
        let have = self.clusters();
        if index < have {
            return self.cluster(index);
        }
        if have == 0 {
            let first = self.fs.take(None)?;
            self.chain = Chain {
                first,
                contiguous: true,
            };
            self.walked = (0, first);
            self.dirty = true;
            return Ok(first);
        }
        let last = self.cluster(have - 1)?;
        let new = self.fs.take(Some(last.wrapping_add(1)))?;
        if !(self.chain.contiguous && new == last.wrapping_add(1)) {
            if self.chain.contiguous {
                // Its run becomes a chain: the links of the clusters it
                // has, which nothing reads until its entry set says so.
                for cluster in self.chain.first..last {
                    self.fs.fat_set(cluster, cluster + 1)?;
                }
                self.chain.contiguous = false;
            }
            self.fs.fat_set(new, FAT_END)?;
            self.fs.fat_set(last, new)?;
            self.walked = (have, new);
        }
        self.dirty = true;
        Ok(new)
    }

    pub(crate) fn write(&mut self, buf: &[u8]) -> Res<usize, IO> {
        if buf.is_empty() {
            return Ok(0);
        }
        let size = self.fs.cluster_size();
        if self.pos > self.valid {
            // What lies between was never written: zeros, as it reads.
            let (to, pos) = (self.pos, self.valid);
            self.pos = pos;
            while self.pos < to {
                let within = self.pos & (size - 1);
                let n = (size - within).min(to - self.pos);
                let cluster = self.cluster_to_write()?;
                self.fs.zero(self.fs.cluster_offset(cluster)? + within, n)?;
                self.pos += n;
                self.valid = self.pos;
                self.len = self.len.max(self.pos);
            }
            self.dirty = true;
        }
        let within = self.pos & (size - 1);
        let n = buf.len().min((size - within) as usize);
        let cluster = self.cluster_to_write()?;
        self.fs
            .write_at(self.fs.cluster_offset(cluster)? + within, &buf[..n])?;
        self.pos += n as u64;
        if self.pos > self.valid {
            self.valid = self.pos;
            self.dirty = true;
        }
        if self.pos > self.len {
            self.len = self.pos;
        }
        Ok(n)
    }

    /// Writes its entry set where it changed, and what the disk keeps.
    pub(crate) fn flush(&mut self) -> Res<(), IO> {
        if self.dirty
            && let Some(loc) = self.loc
        {
            let (chain, len, valid) = (self.chain, self.len, self.valid);
            let first = if len == 0 { 0 } else { chain.first };
            let (mark, attrs, modified) = (self.mark, self.attrs, self.modified);
            self.fs.update(&loc, |set| {
                dir::set_stream(set, first, len, valid, chain.contiguous);
                if let Some(mark) = mark {
                    dir::set_mark(set, mark);
                }
                if let Some(attrs) = attrs {
                    dir::set_attrs(set, attrs);
                }
                if let Some(modified) = modified {
                    set.bytes[12..16].copy_from_slice(&modified.to_le_bytes());
                    set.bytes[21] = 0;
                }
            })?;
            self.dirty = false;
            self.mark = None;
            self.attrs = None;
            self.modified = None;
        }
        self.fs.flush_disk()
    }

    /// Cuts the file off at its position: its entry set first, then the
    /// clusters past it are given back.
    pub(crate) fn truncate(&mut self) -> Res<(), IO> {
        if self.pos >= self.len {
            return Ok(());
        }
        let had = self.clusters();
        let old = self.chain;
        self.len = self.pos;
        self.valid = self.valid.min(self.len);
        let keep = self.clusters();
        // The last cluster kept and the first let go, before the entry set
        // forgets them.
        let cut = match keep {
            0 => Some((None, old.first)),
            keep if keep < had => {
                let last = self.cluster(keep - 1)?;
                self.fs
                    .next_in(old, last)?
                    .map(|first_gone| (Some(last), first_gone))
            }
            _ => None,
        };
        self.dirty = true;
        self.flush()?;
        let Some((last, gone)) = cut else {
            return Ok(());
        };
        if let Some(last) = last.filter(|_| !old.contiguous) {
            self.fs.fat_set(last, FAT_END)?;
        }
        if keep == 0 {
            self.chain = Chain {
                first: 0,
                contiguous: true,
            };
            self.walked = (0, 0);
        }
        self.fs.free(
            Chain {
                first: gone,
                contiguous: old.contiguous,
            },
            had - keep,
        )
    }

    pub(crate) fn seek(&mut self, to: SeekFrom) -> Res<u64, IO> {
        let target = match to {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::End(n) => i128::from(self.len) + i128::from(n),
            SeekFrom::Current(n) => i128::from(self.pos) + i128::from(n),
        };
        if target < 0 {
            return Err(Error::InvalidInput);
        }
        // Not past its end, as a FAT file's position.
        self.pos = u64::try_from(target).unwrap_or(u64::MAX).min(self.len);
        Ok(self.pos)
    }
}

impl<IO: ReadWriteSeek> Drop for XFile<'_, IO> {
    fn drop(&mut self) {
        if self.dirty {
            drop(self.flush());
        }
    }
}
