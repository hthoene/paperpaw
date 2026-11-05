//! Modified for PaperPaw: bounded, versioned metadata intent records.
//!
//! This is only the record codec. The caller must establish the journal's
//! allocation identity, write and flush the body before the header, and
//! retain the record until metadata and retirement writes are durable.
//! A valid checksum is not permission to repair unrelated metadata.

/// One header sector followed by three payload sectors.
pub const RECORD_BYTES: usize = 2048;
/// Bytes in the separately published header.
pub const HEADER_BYTES: usize = 512;
/// Maximum entries in one exFAT entry set.
pub const MAX_CHANGES: usize = 19;
const ENTRY_BYTES: usize = 72;
const MAGIC: &[u8; 8] = b"PPXINT01";
const VERSION: u32 = 1;
const HEADER_SUM: usize = 84;

/// Values checked against the mounted volume and the verified journal file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    /// Volume serial from the boot region.
    pub serial: u32,
    /// Total mounted volume length in bytes.
    pub volume_bytes: u64,
    /// First byte of the cluster heap.
    pub heap_start: u64,
    /// Exclusive end of the cluster heap.
    pub heap_end: u64,
    /// Independently verified start of the journal allocation.
    pub journal_start: u64,
    /// Independently verified journal allocation length.
    pub journal_len: u64,
    /// Nonzero identity of this journal initialization.
    pub epoch: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
/// Metadata operation represented by the record.
pub enum Operation {
    /// Publishes a new entry set.
    Insert = 1,
    /// Changes an existing entry set.
    Update = 2,
    /// Removes an entry set.
    Clear = 3,
}

/// One entry's complete old and intended bytes, never an ownership guess.
pub struct Change<'a> {
    /// Byte offset of the directory entry within the volume.
    pub offset: u64,
    /// Exact entry bytes before the protected operation.
    pub before: &'a [u8; 32],
    /// Exact intended entry bytes after the protected operation.
    pub after: &'a [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Why record validation or current-entry comparison failed.
pub enum RecordError {
    /// Invalid bounds, structure or checksum.
    Malformed,
    /// A format or operation this codec does not understand.
    UnknownVersion,
    /// Recorded volume or journal identity differs from the mount.
    WrongVolume,
    /// Current entry matches neither recorded image.
    Stale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Which recorded image matches an entry read from disk.
pub enum EntryState {
    /// The old entry image is present.
    Before,
    /// The intended entry image is present.
    After,
}

/// A validated record borrowed from caller-owned, bounded workspace.
pub struct Record<'a> {
    bytes: &'a [u8],
    count: usize,
    /// Nonzero transaction sequence, interpreted by the publication protocol.
    pub sequence: u64,
    /// Recorded metadata operation.
    pub operation: Operation,
}

impl Record<'_> {
    /// Number of validated entry changes.
    pub fn len(&self) -> usize {
        self.count
    }
    /// Whether the record has no changes; validated records are nonempty.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Borrows one validated change, or returns None past the record.
    pub fn change(&self, index: usize) -> Option<Change<'_>> {
        if index >= self.count {
            return None;
        }
        let at = HEADER_BYTES + index * ENTRY_BYTES;
        Some(Change {
            offset: get64(self.bytes, at),
            before: self.bytes.get(at + 8..at + 40)?.try_into().ok()?,
            after: self.bytes.get(at + 40..at + 72)?.try_into().ok()?,
        })
    }

    /// An unrelated current entry cannot be made valid by this record.
    /// The caller validates ALL entries before writing ANY of them.
    pub fn state(&self, index: usize, current: &[u8; 32]) -> Result<EntryState, RecordError> {
        let change = self.change(index).ok_or(RecordError::Malformed)?;
        if current == change.after {
            return Ok(EntryState::After);
        }
        if current == change.before {
            return Ok(EntryState::Before);
        }
        Err(RecordError::Stale)
    }
}

/// Encodes a complete candidate without allocation. Validation precedes
/// output mutation. Writing this buffer in one operation is NOT the
/// publication protocol: write/flush the body, then write/flush the header.
pub fn encode(
    out: &mut [u8],
    identity: Identity,
    sequence: u64,
    operation: Operation,
    changes: &[Change<'_>],
) -> Result<(), RecordError> {
    if out.len() != RECORD_BYTES
        || changes.is_empty()
        || changes.len() > MAX_CHANGES
        || sequence == 0
        || !valid_identity(identity)
    {
        return Err(RecordError::Malformed);
    }
    for (i, change) in changes.iter().enumerate() {
        if !valid_offset(change.offset, identity)
            || changes[..i]
                .iter()
                .any(|earlier| earlier.offset == change.offset)
        {
            return Err(RecordError::Malformed);
        }
    }
    out.fill(0);
    out[..8].copy_from_slice(MAGIC);
    put32(out, 8, VERSION);
    out[12] = operation as u8;
    out[13] = changes.len() as u8;
    put64(out, 16, sequence);
    put32(out, 24, identity.serial);
    for (at, value) in [
        (32, identity.volume_bytes),
        (40, identity.heap_start),
        (48, identity.heap_end),
        (56, identity.journal_start),
        (64, identity.journal_len),
        (72, identity.epoch),
    ] {
        put64(out, at, value);
    }
    for (i, change) in changes.iter().enumerate() {
        let at = HEADER_BYTES + i * ENTRY_BYTES;
        put64(out, at, change.offset);
        out[at + 8..at + 40].copy_from_slice(change.before);
        out[at + 40..at + 72].copy_from_slice(change.after);
    }
    let sum = crc(&out[HEADER_BYTES..]);
    put32(out, 80, sum);
    let sum = header_crc(&out[..HEADER_BYTES]);
    put32(out, HEADER_SUM, sum);
    Ok(())
}

/// Unknown versions are distinguished from damaged known records. Neither
/// may authorize replay. A zero header is an unpublished/retired record.
pub fn decode(bytes: &[u8], expected: Identity) -> Result<Option<Record<'_>>, RecordError> {
    if bytes.len() != RECORD_BYTES {
        return Err(RecordError::Malformed);
    }
    if bytes[..HEADER_BYTES].iter().all(|b| *b == 0) {
        return Ok(None);
    }
    if &bytes[..8] != MAGIC || get32(bytes, 8) != VERSION {
        return Err(RecordError::UnknownVersion);
    }
    if get32(bytes, HEADER_SUM) != header_crc(&bytes[..HEADER_BYTES])
        || get32(bytes, 80) != crc(&bytes[HEADER_BYTES..])
    {
        return Err(RecordError::Malformed);
    }
    let count = usize::from(bytes[13]);
    let sequence = get64(bytes, 16);
    if count == 0
        || count > MAX_CHANGES
        || sequence == 0
        || bytes[14..16].iter().any(|b| *b != 0)
        || bytes[28..32].iter().any(|b| *b != 0)
        || bytes[88..HEADER_BYTES].iter().any(|b| *b != 0)
        || bytes[HEADER_BYTES + count * ENTRY_BYTES..]
            .iter()
            .any(|b| *b != 0)
    {
        return Err(RecordError::Malformed);
    }
    let identity = Identity {
        serial: get32(bytes, 24),
        volume_bytes: get64(bytes, 32),
        heap_start: get64(bytes, 40),
        heap_end: get64(bytes, 48),
        journal_start: get64(bytes, 56),
        journal_len: get64(bytes, 64),
        epoch: get64(bytes, 72),
    };
    if !valid_identity(identity) {
        return Err(RecordError::Malformed);
    }
    if identity != expected {
        return Err(RecordError::WrongVolume);
    }
    let operation = match bytes[12] {
        1 => Operation::Insert,
        2 => Operation::Update,
        3 => Operation::Clear,
        _ => return Err(RecordError::UnknownVersion),
    };
    for i in 0..count {
        let offset = get64(bytes, HEADER_BYTES + i * ENTRY_BYTES);
        if !valid_offset(offset, identity)
            || (0..i).any(|j| get64(bytes, HEADER_BYTES + j * ENTRY_BYTES) == offset)
        {
            return Err(RecordError::Malformed);
        }
    }
    Ok(Some(Record {
        bytes,
        count,
        sequence,
        operation,
    }))
}

fn valid_identity(id: Identity) -> bool {
    id.epoch != 0
        && id.heap_start < id.heap_end
        && id.heap_end <= id.volume_bytes
        && id.journal_start >= id.heap_start
        && id.journal_start % 512 == 0
        && id.journal_len >= (2 * RECORD_BYTES) as u64
        && id.journal_len % 512 == 0
        && id
            .journal_start
            .checked_add(id.journal_len)
            .is_some_and(|end| end <= id.heap_end)
}

fn valid_offset(offset: u64, id: Identity) -> bool {
    let Some(end) = offset.checked_add(32) else {
        return false;
    };
    let Some(journal_end) = id.journal_start.checked_add(id.journal_len) else {
        return false;
    };
    offset % 32 == 0
        && offset >= id.heap_start
        && end <= id.heap_end
        && (end <= id.journal_start || offset >= journal_end)
}

fn get32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}
fn get64(bytes: &[u8], at: usize) -> u64 {
    u64::from(get32(bytes, at)) | (u64::from(get32(bytes, at + 4)) << 32)
}
fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn put64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

fn crc(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc = crc_byte(crc, byte);
    }
    !crc
}
fn header_crc(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for (i, &byte) in bytes.iter().enumerate() {
        crc = crc_byte(
            crc,
            if (HEADER_SUM..HEADER_SUM + 4).contains(&i) {
                0
            } else {
                byte
            },
        );
    }
    !crc
}
fn crc_byte(mut crc: u32, byte: u8) -> u32 {
    crc ^= u32::from(byte);
    for _ in 0..8 {
        crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1)));
    }
    crc
}
