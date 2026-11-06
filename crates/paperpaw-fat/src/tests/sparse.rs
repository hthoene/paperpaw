//! Test images keep only nonzero blocks, including when a mount is leaked.
//! Logical size and all byte offsets stay identical to a dense card image.
use std::collections::BTreeMap;
use std::io::{self, Read, Seek, SeekFrom, Write};

use super::BLOCK;

#[derive(Debug)]
pub(super) struct SparseDisk {
    blocks: BTreeMap<u64, [u8; BLOCK]>,
    len: u64,
    at: u64,
}

impl SparseDisk {
    pub(super) fn empty(len: usize) -> Self {
        Self {
            blocks: BTreeMap::new(),
            len: len as u64,
            at: 0,
        }
    }

    pub(super) fn from_bytes(bytes: &[u8]) -> Self {
        let mut disk = Self::empty(bytes.len());
        for (index, bytes) in bytes.chunks(BLOCK).enumerate() {
            if bytes.iter().any(|&byte| byte != 0) {
                let mut block = [0; BLOCK];
                block[..bytes.len()].copy_from_slice(bytes);
                disk.blocks.insert(index as u64, block);
            }
        }
        disk
    }
}

impl Read for SparseDisk {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let len = out
            .len()
            .min(usize::try_from(self.len.saturating_sub(self.at)).unwrap_or(usize::MAX));
        let mut done = 0;
        while done < len {
            let index = self.at / BLOCK as u64;
            let offset = (self.at % BLOCK as u64) as usize;
            let count = (len - done).min(BLOCK - offset);
            match self.blocks.get(&index) {
                Some(block) => {
                    out[done..done + count].copy_from_slice(&block[offset..offset + count])
                }
                None => out[done..done + count].fill(0),
            }
            self.at += count as u64;
            done += count;
        }
        Ok(done)
    }
}

impl Write for SparseDisk {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let end = self
            .at
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        let mut done = 0;
        while done < bytes.len() {
            let index = self.at / BLOCK as u64;
            let offset = (self.at % BLOCK as u64) as usize;
            let count = (bytes.len() - done).min(BLOCK - offset);
            let block = self.blocks.entry(index).or_insert([0; BLOCK]);
            block[offset..offset + count].copy_from_slice(&bytes[done..done + count]);
            if block.iter().all(|&byte| byte == 0) {
                self.blocks.remove(&index);
            }
            self.at += count as u64;
            done += count;
        }
        self.len = self.len.max(end);
        Ok(done)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for SparseDisk {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let at = match from {
            SeekFrom::Start(at) => i128::from(at),
            SeekFrom::Current(by) => i128::from(self.at) + i128::from(by),
            SeekFrom::End(by) => i128::from(self.len) + i128::from(by),
        };
        self.at = u64::try_from(at).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        Ok(self.at)
    }
}

#[test]
fn sparse_bytes_match_a_dense_cursor_across_blocks_holes_and_growth() {
    let mut bytes = std::vec![0; BLOCK * 4 + 7];
    bytes[BLOCK - 2..BLOCK + 3].copy_from_slice(b"hello");
    let mut sparse = SparseDisk::from_bytes(&bytes);
    assert_eq!(sparse.blocks.len(), 2);
    let mut dense = io::Cursor::new(bytes);
    for at in [0, BLOCK - 3, BLOCK * 3, BLOCK * 5] {
        assert_eq!(
            sparse.seek(SeekFrom::Start(at as u64)).unwrap(),
            dense.seek(SeekFrom::Start(at as u64)).unwrap()
        );
        sparse.write_all(b"changed across blocks").unwrap();
        dense.write_all(b"changed across blocks").unwrap();
    }
    sparse.seek(SeekFrom::Start(0)).unwrap();
    let mut actual = std::vec::Vec::new();
    sparse.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, dense.into_inner());
    assert!(sparse.seek(SeekFrom::Start(u64::MAX)).is_ok());
    assert!(sparse.write_all(b"overflow").is_err());
    assert_eq!(
        sparse.seek(SeekFrom::End(-1)).unwrap(),
        actual.len() as u64 - 1
    );
    assert!(sparse.seek(SeekFrom::Current(-i64::MAX)).is_err());
}

#[test]
fn clearing_a_sparse_block_releases_its_storage() {
    let mut sparse = SparseDisk::from_bytes(&[1; BLOCK]);
    sparse.write_all(&[0; BLOCK]).unwrap();
    assert!(sparse.blocks.is_empty());
}
