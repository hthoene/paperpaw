//! Committed declaration bytes. Submitted addresses are never dereferenced.
use super::{
    registrations::{REGISTRATION_MAX_RECORDS, Record, RecordBatch},
    staged::{DECLARATIONS_MAX_BYTES, IMAGE_DECLARATIONS_MAX, IMAGE_INDEX_MAX_BYTES},
};
use alloc::{boxed::Box, vec::Vec};
use core::mem::size_of;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE};

#[derive(Clone, Copy)]
pub(crate) struct ImageRange {
    pub(crate) base: usize,
    pub(crate) offset: u32,
    pub(crate) len: u32,
}
pub(crate) struct RawBatch {
    pub(crate) submitted: usize,
    pub(crate) bytes: Box<[u8]>,
    pub(crate) image: Option<ImageRange>,
    pub(crate) offsets: [u16; REGISTRATION_MAX_RECORDS],
    pub(crate) count: usize,
    entries: usize,
}
impl RawBatch {
    pub(crate) fn view<'a>(&'a self, image: Option<&'a [u8]>) -> Result<&'a [u8], i32> {
        let Some(range) = self.image else {
            return Ok(&self.bytes);
        };
        let image = image
            .filter(|image| image.as_ptr() as usize == range.base)
            .ok_or(ERR_INVALID)?;
        let start = range.offset as usize;
        start
            .checked_add(range.len as usize)
            .and_then(|end| image.get(start..end))
            .ok_or(ERR_INVALID)
    }
}
#[derive(Default)]
pub(crate) struct RawDeclarations {
    pub(crate) batches: Vec<RawBatch>,
    bytes: usize,
    pub(crate) image_entries: usize,
    pub(crate) index_bytes: usize,
}
impl RawDeclarations {
    /// Image inputs retain only offsets. Runtime inputs are copied before return.
    pub(crate) fn prepare(
        &mut self,
        image: Option<&[u8]>,
        wire: Option<&[u8]>,
        batch: &RecordBatch<'_>,
    ) -> Result<Option<RawBatch>, i32> {
        let Some(wire) = wire else {
            return Ok(None);
        };
        let entries = batch.iter().filter(is_declaration).count();
        if entries == 0 {
            return Ok(None);
        }
        let range = image.and_then(|image| image_range(image, wire));
        if range.is_some() {
            if self
                .image_entries
                .checked_add(entries)
                .is_none_or(|n| n > IMAGE_DECLARATIONS_MAX)
                || self
                    .index_bytes
                    .checked_add(index_cost(entries))
                    .is_none_or(|n| n > IMAGE_INDEX_MAX_BYTES)
            {
                return Err(ERR_NO_SPACE);
            }
        } else {
            self.retained_with(wire.len())?;
        }
        let offsets = offsets(wire, batch.len())?;
        let mut copy = Vec::new();
        if range.is_none() {
            copy.try_reserve_exact(wire.len())
                .map_err(|_| ERR_NO_MEMORY)?;
            copy.extend_from_slice(wire);
        }
        // Exact capacity keeps the image index budget independent of growth slack.
        self.batches
            .try_reserve_exact(1)
            .map_err(|_| ERR_NO_MEMORY)?;
        Ok(Some(RawBatch {
            submitted: wire.as_ptr() as usize,
            bytes: copy.into_boxed_slice(),
            image: range,
            offsets,
            count: batch.len(),
            entries,
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
        self.bytes
            + pending
                .filter(|batch| batch.image.is_none())
                .map_or(0, |batch| size_of::<RawBatch>() + batch.bytes.len())
    }
    pub(crate) fn commit(&mut self, batch: Option<RawBatch>) {
        if let Some(batch) = batch {
            if batch.image.is_some() {
                self.image_entries += batch.entries;
                self.index_bytes += index_cost(batch.entries);
            } else {
                self.bytes += size_of::<RawBatch>() + batch.bytes.len();
            }
            self.batches.push(batch);
        }
    }
    pub(crate) fn visit<'a>(
        &'a self,
        plugin: &str,
        image: Option<&'a [u8]>,
        visit: &mut dyn FnMut(Record<'a>),
    ) -> Result<(), i32> {
        for batch in &self.batches {
            let decoded = super::registrations::records_decode(batch.view(image)?)?;
            super::declarations::validate_batch(plugin, &decoded)?;
            for record in decoded.iter() {
                visit(record);
            }
        }
        Ok(())
    }
}
pub(crate) fn is_declaration(record: &Record<'_>) -> bool {
    matches!(
        record,
        Record::SettingGroup(_)
            | Record::SettingSpec(_)
            | Record::SettingExtra(_)
            | Record::ResourceHyphenation(_)
            | Record::ResourceFontFace(_)
            | Record::ResourceFontExtension(_)
    )
}
fn index_cost(entries: usize) -> usize {
    size_of::<RawBatch>() + entries * 4
}
fn image_range(image: &[u8], wire: &[u8]) -> Option<ImageRange> {
    let base = image.as_ptr() as usize;
    let offset = (wire.as_ptr() as usize).checked_sub(base)?;
    image.get(offset..offset.checked_add(wire.len())?)?;
    Some(ImageRange {
        base,
        offset: u32::try_from(offset).ok()?,
        len: u32::try_from(wire.len()).ok()?,
    })
}
/// The generated decoder validates framing; only offsets are collected here.
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
