//! Modified for PaperPaw: serialized publication and replay of bounded intents.
//!
//! The adapter must independently verify the journal allocation and target
//! ownership. This module neither creates a journal nor infers ownership from
//! allocation bits. Do not expose filesystem mutation before bootstrap/replay.

use super::intent::{self, Change, HEADER_BYTES, Identity, Operation, RECORD_BYTES, RecordError};

/// I/O over a separately verified journal and the mounted volume.
/// Implementations must preserve write/flush order and must not recursively
/// journal writes to the journal itself. Reads observe preceding writes.
pub trait IntentIo {
    /// Device or identity-validation failure.
    type Error;
    /// Reads one complete journal slot into the caller's workspace.
    fn read_slot(&mut self, slot: usize, out: &mut [u8]) -> Result<(), Self::Error>;
    /// Writes a range within one slot; offset zero is its header.
    fn write_slot(&mut self, slot: usize, offset: usize, data: &[u8]) -> Result<(), Self::Error>;
    /// Validates target identity/ownership against independent mounted state.
    /// A record checksum or allocated first cluster alone is not sufficient.
    fn validate_target(&mut self, change: &Change<'_>) -> Result<(), Self::Error>;
    /// Reads a complete directory entry without changing it.
    fn read_entry(&mut self, offset: u64, out: &mut [u8; 32]) -> Result<(), Self::Error>;
    /// Writes exactly the recorded entry, preserving adjacent bytes.
    fn write_entry(&mut self, offset: u64, data: &[u8; 32]) -> Result<(), Self::Error>;
    /// Makes all preceding writes durable before returning success.
    fn flush(&mut self) -> Result<(), Self::Error>;
}

/// No error permits silently falling back to an older intent.
#[derive(Debug, PartialEq, Eq)]
pub enum TransactionError<E> {
    /// I/O or independent target validation failed.
    Io(E),
    /// Record contents failed validation.
    Record(RecordError),
    /// Multiple prepared records or inconsistent sequencing.
    Ambiguous,
    /// Sequence numbers never wrap within an epoch.
    Exhausted,
}

#[derive(Clone, Copy)]
struct Slot {
    sequence: u64,
    prepared: bool,
}

fn scan<I: IntentIo>(
    io: &mut I,
    identity: Identity,
    work: &mut [u8],
) -> Result<[Slot; 2], TransactionError<I::Error>> {
    let mut slots = [Slot {
        sequence: 0,
        prepared: false,
    }; 2];
    for (index, slot) in slots.iter_mut().enumerate() {
        io.read_slot(index, work).map_err(TransactionError::Io)?;
        if let Some(record) = intent::decode(work, identity).map_err(TransactionError::Record)? {
            *slot = Slot {
                sequence: record.sequence,
                prepared: !record.applied,
            };
        }
    }
    if slots.iter().filter(|slot| slot.prepared).count() > 1
        || (slots[0].sequence != 0 && slots[0].sequence == slots[1].sequence)
        || (0..2).any(|i| slots[i].prepared && slots[i].sequence < slots[1 - i].sequence)
    {
        return Err(TransactionError::Ambiguous);
    }
    Ok(slots)
}

/// Replays the single newest prepared transaction, if any. Every target is
/// checked before the first mutation. Errors retain the prepared record;
/// callers must not start another operation until replay succeeds.
pub fn recover<I: IntentIo>(
    io: &mut I,
    identity: Identity,
    work: &mut [u8],
) -> Result<(), TransactionError<I::Error>> {
    let slots = scan(io, identity, work)?;
    if let Some(slot) = slots.iter().position(|s| s.prepared) {
        io.read_slot(slot, work).map_err(TransactionError::Io)?;
        let record = intent::decode(work, identity)
            .map_err(TransactionError::Record)?
            .ok_or(TransactionError::Ambiguous)?;
        for index in 0..record.len() {
            let change = record.change(index).ok_or(TransactionError::Ambiguous)?;
            io.validate_target(&change).map_err(TransactionError::Io)?;
            let mut current = [0; 32];
            io.read_entry(change.offset, &mut current)
                .map_err(TransactionError::Io)?;
            record
                .state(index, &current)
                .map_err(TransactionError::Record)?;
        }
        // Secondary entries first, primary last, matching exFAT publication.
        for index in (0..record.len()).rev() {
            let change = record.change(index).ok_or(TransactionError::Ambiguous)?;
            io.write_entry(change.offset, change.after)
                .map_err(TransactionError::Io)?;
        }
        io.flush().map_err(TransactionError::Io)?;
        intent::applied_header(work, identity).map_err(TransactionError::Record)?;
        io.write_slot(slot, 0, &work[..HEADER_BYTES])
            .map_err(TransactionError::Io)?;
        io.flush().map_err(TransactionError::Io)?;
    }
    Ok(())
}

/// Publishes one transaction after all prior work is resolved. The caller
/// must flush allocation/data dependencies before calling this function.
/// Uses exactly RECORD_BYTES of caller-owned workspace and no allocations.
pub fn execute<I: IntentIo>(
    io: &mut I,
    identity: Identity,
    operation: Operation,
    changes: &[Change<'_>],
    work: &mut [u8],
) -> Result<(), TransactionError<I::Error>> {
    if work.len() != RECORD_BYTES {
        return Err(TransactionError::Record(RecordError::Malformed));
    }
    recover(io, identity, work)?;
    let slots = scan(io, identity, work)?;
    let latest = slots[0].sequence.max(slots[1].sequence);
    let sequence = latest.checked_add(1).ok_or(TransactionError::Exhausted)?;
    let slot = usize::from(slots[0].sequence > slots[1].sequence);
    intent::encode(work, identity, sequence, operation, changes)
        .map_err(TransactionError::Record)?;
    // Changes were prepared before replay; verify they are still current.
    for change in changes {
        io.validate_target(change).map_err(TransactionError::Io)?;
        let mut current = [0; 32];
        io.read_entry(change.offset, &mut current)
            .map_err(TransactionError::Io)?;
        if &current != change.before {
            return Err(TransactionError::Record(RecordError::Stale));
        }
    }
    // Invalidate before overwriting a slot's old body. A cut during body
    // population leaves no published transaction in this slot.
    io.write_slot(slot, 0, &[0; HEADER_BYTES])
        .map_err(TransactionError::Io)?;
    io.flush().map_err(TransactionError::Io)?;
    io.write_slot(slot, HEADER_BYTES, &work[HEADER_BYTES..])
        .map_err(TransactionError::Io)?;
    io.flush().map_err(TransactionError::Io)?;
    io.write_slot(slot, 0, &work[..HEADER_BYTES])
        .map_err(TransactionError::Io)?;
    io.flush().map_err(TransactionError::Io)?;
    recover(io, identity, work)
}
