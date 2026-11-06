//! Committed declaration bytes. Submitted addresses are never dereferenced.
use super::{
    declaration_index::{NameEntry, key, name_hash, same_name},
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
    pub(crate) bytes: Box<[u8]>,
    pub(crate) image: Option<ImageRange>,
    pub(crate) offsets: [u16; REGISTRATION_MAX_RECORDS],
    pub(crate) entries: u16,
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
pub(crate) struct PreparedRaw {
    pub(crate) batch: RawBatch,
    index: [NameEntry; REGISTRATION_MAX_RECORDS],
    len: usize,
    counts: [usize; 6],
}

#[derive(Default)]
pub(crate) struct RawDeclarations {
    pub(crate) batches: Vec<RawBatch>,
    bytes: usize,
    owned_counts: [usize; 6],
    pub(crate) index: Vec<NameEntry>,
    sorted: bool,
    #[cfg(test)]
    pub(crate) force_hash: Option<u32>,
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
    ) -> Result<Option<PreparedRaw>, i32> {
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
            self.retained_with(wire.len(), entries)?;
        }
        let offsets = offsets(wire, batch.len())?;
        let mut index = [NameEntry::default(); REGISTRATION_MAX_RECORDS];
        let mut len = 0;
        for (record, value) in batch.iter().enumerate() {
            if let Some((kind, name)) = key(value) {
                index[len] = NameEntry {
                    hash: self.hash(kind, name),
                    batch: u16::try_from(self.batches.len()).map_err(|_| ERR_NO_SPACE)?,
                    record_kind: (u16::from(kind) << 3)
                        | u16::try_from(record).map_err(|_| ERR_INVALID)?,
                };
                len += 1;
            }
        }
        let mut copy = Vec::new();
        if range.is_none() {
            copy.try_reserve_exact(wire.len())
                .map_err(|_| ERR_NO_MEMORY)?;
            copy.extend_from_slice(wire);
        }
        self.batches
            .try_reserve_exact(1)
            .map_err(|_| ERR_NO_MEMORY)?;
        self.index
            .try_reserve_exact(entries)
            .map_err(|_| ERR_NO_MEMORY)?;
        Ok(Some(PreparedRaw {
            batch: RawBatch {
                bytes: copy.into_boxed_slice(),
                image: range,
                offsets,
                entries: u16::try_from(entries).map_err(|_| ERR_NO_SPACE)?,
            },
            index,
            len,
            counts: counts(batch),
        }))
    }
    pub(crate) fn check_counts(
        &self,
        decoded: &super::staged::Declarations,
        batch: &RecordBatch<'_>,
        pending: Option<&PreparedRaw>,
    ) -> Result<(), i32> {
        use super::staged::{
            EXTRAS_MAX, FONT_EXTENSIONS_MAX, FONT_FACES_MAX, GROUPS_MAX, HYPHENATIONS_MAX,
            SPECS_MAX,
        };
        if pending.is_some_and(|p| p.batch.image.is_some()) {
            return Ok(());
        }
        let old = [
            decoded.groups().len(),
            decoded.specs().len(),
            decoded.extras().len(),
            decoded.hyphenations().len(),
            decoded.font_faces().len(),
            decoded.font_extensions().len(),
        ];
        let limits = [
            GROUPS_MAX,
            SPECS_MAX,
            EXTRAS_MAX,
            HYPHENATIONS_MAX,
            FONT_FACES_MAX,
            FONT_EXTENSIONS_MAX,
        ];
        if old
            .into_iter()
            .zip(self.owned_counts)
            .zip(counts(batch))
            .zip(limits)
            .any(|(((decoded, raw), new), limit)| decoded + raw + new > limit)
        {
            return Err(ERR_NO_SPACE);
        }
        Ok(())
    }
    fn retained_with(&self, len: usize, entries: usize) -> Result<usize, i32> {
        self.bytes
            .checked_add(index_cost(entries))
            .and_then(|n| n.checked_add(len))
            .filter(|n| *n <= DECLARATIONS_MAX_BYTES)
            .ok_or(ERR_NO_SPACE)
    }
    pub(crate) fn retained(&self, pending: Option<&PreparedRaw>) -> usize {
        self.bytes
            + pending
                .filter(|p| p.batch.image.is_none())
                .map_or(0, |p| index_cost(p.len) + p.batch.bytes.len())
    }
    pub(crate) fn commit(&mut self, pending: Option<PreparedRaw>) {
        if let Some(pending) = pending {
            let batch = pending.batch;
            if batch.image.is_some() {
                self.image_entries += pending.len;
                self.index_bytes += index_cost(pending.len);
            } else {
                self.bytes += index_cost(pending.len) + batch.bytes.len();
                for (total, count) in self.owned_counts.iter_mut().zip(pending.counts) {
                    *total += count;
                }
            }
            self.batches.push(batch);
            self.index.extend_from_slice(&pending.index[..pending.len]);
            self.sorted = false;
        }
    }
    fn hash(&self, kind: u8, name: &str) -> u32 {
        #[cfg(test)]
        if let Some(hash) = self.force_hash {
            return hash;
        }
        name_hash(kind, name)
    }
    pub(crate) fn sort_index(&mut self) {
        self.index.sort_unstable_by_key(|entry| entry.order());
        self.sorted = true;
    }
    pub(crate) fn record<'a>(
        &'a self,
        image: Option<&'a [u8]>,
        entry: NameEntry,
    ) -> Result<Record<'a>, i32> {
        let batch = self
            .batches
            .get(usize::from(entry.batch))
            .ok_or(ERR_INVALID)?;
        let bytes = batch.view(image)?;
        let offset = usize::from(*batch.offsets.get(entry.record()).ok_or(ERR_INVALID)?);
        let (record, _) = super::registrations::record_decode_prefix(
            bytes.get(offset..).ok_or(ERR_INVALID)?
        )?;
        let (kind, name) = key(record).ok_or(ERR_INVALID)?;
        if kind != entry.kind() || self.hash(kind, name) != entry.hash {
            return Err(ERR_INVALID);
        }
        Ok(record)
    }
    pub(crate) fn find<'a>(
        &'a self,
        image: Option<&'a [u8]>,
        kind: u8,
        name: &str,
    ) -> Result<Option<Record<'a>>, i32> {
        if !self.sorted {
            return Err(ERR_INVALID);
        }
        let wanted = (kind, self.hash(kind, name));
        let start = self.index.partition_point(|entry| entry.order() < wanted);
        for entry in self.index[start..]
            .iter()
            .take_while(|entry| entry.order() == wanted)
        {
            let record = self.record(image, *entry)?;
            if key(record).is_some_and(|(_, actual)| same_name(kind, actual, name)) {
                return Ok(Some(record));
            }
        }
        Ok(None)
    }
    pub(crate) fn check_duplicates(&self, image: Option<&[u8]>) -> Result<(), i32> {
        if !self.sorted {
            return Err(ERR_INVALID);
        }
        for (at, entry) in self.index.iter().enumerate() {
            // Full byte comparisons are needed only within an equal-hash run.
            for earlier in self.index[..at]
                .iter()
                .rev()
                .take_while(|earlier| earlier.order() == entry.order())
            {
                let a = key(self.record(image, *entry)?).ok_or(ERR_INVALID)?;
                let b = key(self.record(image, *earlier)?).ok_or(ERR_INVALID)?;
                if same_name(a.0, a.1, b.1) {
                    return Err(ERR_INVALID);
                }
            }
        }
        Ok(())
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
fn counts(batch: &RecordBatch<'_>) -> [usize; 6] {
    let mut counts = [0; 6];
    for record in batch.iter() {
        let kind = match record {
            Record::SettingGroup(_) => 0,
            Record::SettingSpec(_) => 1,
            Record::SettingExtra(_) => 2,
            Record::ResourceHyphenation(_) => 3,
            Record::ResourceFontFace(_) => 4,
            Record::ResourceFontExtension(_) => 5,
            _ => continue,
        };
        counts[kind] += 1;
    }
    counts
}
fn index_cost(entries: usize) -> usize {
    size_of::<RawBatch>() + entries * size_of::<NameEntry>()
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
pub(crate) fn offsets(wire: &[u8], count: usize) -> Result<[u16; REGISTRATION_MAX_RECORDS], i32> {
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
