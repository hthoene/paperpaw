//! Construction snapshots of committed wire batches. Addresses are metadata.
use super::{
    registrations::{REGISTRATION_MAX_RECORDS, Record, RecordBatch},
    staged::DECLARATIONS_MAX_BYTES,
};
use alloc::{boxed::Box, vec::Vec};
use core::mem::size_of;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE};

pub(crate) struct RawBatch {
    pub(crate) submitted: usize,
    pub(crate) bytes: Box<[u8]>,
    pub(crate) offsets: [u16; REGISTRATION_MAX_RECORDS],
    pub(crate) count: usize,
}
#[derive(Default)]
pub(crate) struct RawDeclarations {
    pub(crate) batches: Vec<RawBatch>,
    bytes: usize,
}
impl RawDeclarations {
    /// Reserve publication storage before decoded declarations are staged.
    pub(crate) fn prepare(
        &mut self,
        wire: Option<&[u8]>,
        batch: &RecordBatch<'_>,
    ) -> Result<Option<RawBatch>, i32> {
        let Some(wire) = wire else {
            return Ok(None);
        };
        if !batch.iter().any(|record| {
            matches!(
                record,
                Record::SettingGroup(_)
                    | Record::SettingSpec(_)
                    | Record::SettingExtra(_)
                    | Record::ResourceHyphenation(_)
                    | Record::ResourceFontFace(_)
                    | Record::ResourceFontExtension(_)
            )
        }) {
            return Ok(None);
        }
        self.retained_with(wire.len())?;
        let offsets = offsets(wire, batch.len())?;
        let mut copy = Vec::new();
        copy.try_reserve_exact(wire.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        copy.extend_from_slice(wire);
        self.batches
            .try_reserve_exact(1)
            .map_err(|_| ERR_NO_MEMORY)?;
        Ok(Some(RawBatch {
            submitted: wire.as_ptr() as usize,
            bytes: copy.into_boxed_slice(),
            offsets,
            count: batch.len(),
        }))
    }
    fn retained_with(&self, len: usize) -> Result<usize, i32> {
        self.bytes
            .checked_add(size_of::<RawBatch>())
            .and_then(|n| n.checked_add(len))
            .filter(|n| *n <= DECLARATIONS_MAX_BYTES)
            .ok_or(ERR_NO_SPACE)
    }
    pub(crate) fn retained(&self, pending: Option<&RawBatch>) -> usize {
        self.bytes + pending.map_or(0, |batch| size_of::<RawBatch>() + batch.bytes.len())
    }
    pub(crate) fn commit(&mut self, batch: Option<RawBatch>) {
        if let Some(batch) = batch {
            self.bytes += size_of::<RawBatch>() + batch.bytes.len();
            self.batches.push(batch);
        }
    }
}

/// The generated decoder already validated framing. Keep offsets without
/// changing the generated codec or retaining any decoded native fields.
fn offsets(wire: &[u8], count: usize) -> Result<[u16; REGISTRATION_MAX_RECORDS], i32> {
    let mut offsets = [0; REGISTRATION_MAX_RECORDS];
    let mut cursor = 0;
    let encoded_count = varint(wire, &mut cursor)?;
    if encoded_count as usize != count || count > offsets.len() {
        return Err(ERR_INVALID);
    }
    for offset in offsets.iter_mut().take(count) {
        *offset = u16::try_from(cursor).map_err(|_| ERR_INVALID)?;
        varint(wire, &mut cursor)?;
        let len = usize::try_from(varint(wire, &mut cursor)?).map_err(|_| ERR_INVALID)?;
        cursor = cursor
            .checked_add(len)
            .filter(|end| *end <= wire.len())
            .ok_or(ERR_INVALID)?;
    }
    if cursor != wire.len() {
        return Err(ERR_INVALID);
    }
    Ok(offsets)
}
fn varint(wire: &[u8], cursor: &mut usize) -> Result<u32, i32> {
    let mut value = 0;
    for shift in (0..=28).step_by(7) {
        let byte = *wire.get(*cursor).ok_or(ERR_INVALID)?;
        *cursor += 1;
        if shift == 28 && byte > 15 {
            return Err(ERR_INVALID);
        }
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(ERR_INVALID)
}
