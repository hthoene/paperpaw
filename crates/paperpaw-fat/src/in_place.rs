//! [`Storage::write_at`]: bytes of a file written where they are, for a
//! file kept in slots of fixed size (a book's reading place, #305). Within
//! the file nothing but its data changes: no cluster is taken, the entry
//! stays as it is, and a slot that is one block of the card is one block
//! written; cut off, the block holds its old bytes or its new ones. See
//! the [crate documentation](crate).

use super::*;

impl<IO: ReadWriteSeek + 'static> FatStorage<IO> {
    /// [`Storage::write_at`], within the file or at its end (it grows
    /// then, as by an append written back at once). Past its end it is
    /// `Unsupported`: nothing here fills the gap.
    pub(super) fn write_in_place(
        &self,
        path: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<(), StorageError> {
        self.settle_moves()?;
        let result = self.with_open(path, false, |o| {
            // What was appended is written back first: its size is in the
            // entry, no cluster taken ahead is left behind it.
            o.flush()?;
            if offset > o.len {
                return Err(StorageError::Unsupported);
            }
            if o.file.seek(SeekFrom::Start(offset)).map_err(error)? != offset {
                return Err(StorageError::Io);
            }
            let written = o.file.write_all(data).map_err(error);
            // As far as it came, where the card is full.
            let end = o.file.seek(SeekFrom::Current(0)).map_err(error)?;
            o.len = o.len.max(end);
            // The entry too, where the file grew.
            let flushed = o.file.flush().map_err(error);
            written.and(flushed)
        });
        self.written(result)
    }
}
