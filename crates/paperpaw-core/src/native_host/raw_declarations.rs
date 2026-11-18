//! Committed declaration bytes. Submitted addresses are never dereferenced.
use super::{
    declaration_index::{
        NameEntry, composite_hash, key, matches_parts, record_hash, same_record_name,
    },
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
    #[cfg(test)]
    pub(crate) fn prepare(
        &mut self,
        image: Option<&[u8]>,
        wire: Option<&[u8]>,
        batch: &RecordBatch<'_>,
    ) -> Result<Option<PreparedRaw>, i32> {
        self.prepare_retaining(image, wire, batch, 0)
    }
    /// Image inputs retain only offsets. Runtime inputs are copied before return.
    pub(crate) fn prepare_retaining(
        &mut self,
        image: Option<&[u8]>,
        wire: Option<&[u8]>,
        batch: &RecordBatch<'_>,
        external: usize,
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
            if let Some((kind, _)) = key(value) {
                index[len] = NameEntry::new(
                    self.record_hash(value)?,
                    u16::try_from(self.batches.len()).map_err(|_| ERR_NO_SPACE)?,
                    kind,
                    record,
                    matches!(value, Record::ResourceFontExtension(_)),
                )?;
                len += 1;
            }
        }
        self.reserve_indexes(entries, range.is_some(), wire.len(), external)?;
        let mut copy = Vec::new();
        if range.is_none() {
            copy.try_reserve_exact(wire.len())
                .map_err(|_| ERR_NO_MEMORY)?;
            copy.extend_from_slice(wire);
        }
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
    /// Spare capacity must fit every active pool. This conservatively charges
    /// shared spare slots to both pools instead of hiding them in either one.
    fn reserve_indexes(
        &mut self,
        entries: usize,
        image: bool,
        wire: usize,
        external: usize,
    ) -> Result<(), i32> {
        let added = index_cost(entries);
        let image_bytes = self
            .index_bytes
            .checked_add(if image { added } else { 0 })
            .ok_or(ERR_NO_SPACE)?;
        let owned_bytes = self
            .bytes
            .checked_add(external)
            .and_then(|n| n.checked_add(if image { 0 } else { added }))
            .and_then(|n| n.checked_add(if image { 0 } else { wire }))
            .ok_or(ERR_NO_SPACE)?;
        let mut spare = usize::MAX;
        if image_bytes != 0 {
            spare = spare.min(
                IMAGE_INDEX_MAX_BYTES
                    .checked_sub(image_bytes)
                    .ok_or(ERR_NO_SPACE)?,
            );
        }
        if owned_bytes != 0 {
            spare = spare.min(
                DECLARATIONS_MAX_BYTES
                    .checked_sub(owned_bytes)
                    .ok_or(ERR_NO_SPACE)?,
            );
        }
        let batches = self.batches.len().checked_add(1).ok_or(ERR_NO_SPACE)?;
        let names = self.index.len().checked_add(entries).ok_or(ERR_NO_SPACE)?;
        let budget = index_size(batches, names)?
            .checked_add(spare)
            .ok_or(ERR_NO_SPACE)?;
        let [batch_cap, name_cap] = growth(
            [self.batches.capacity(), self.index.capacity()],
            [batches, names],
            budget,
        )?;
        self.batches
            .try_reserve_exact(batch_cap - self.batches.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        self.index
            .try_reserve_exact(name_cap - self.index.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        // Allocators may grant more than requested. Refuse if that exceeds the budget.
        // The larger capacity remains as charged spare until the failed
        // construction drops; no batch or index entries are published.
        if index_size(self.batches.capacity(), self.index.capacity())? > budget {
            return Err(ERR_NO_SPACE);
        }
        Ok(())
    }
    fn spare(&self, pending: Option<&PreparedRaw>) -> usize {
        let batches = self.batches.len() + usize::from(pending.is_some());
        let names = self.index.len() + pending.map_or(0, |p| p.len);
        self.batches.capacity().saturating_sub(batches) * size_of::<RawBatch>()
            + self.index.capacity().saturating_sub(names) * size_of::<NameEntry>()
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
        self.spare(pending)
            + self.bytes
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
    fn hash(&self, kind: u8, parts: &[&str]) -> u32 {
        #[cfg(test)]
        if let Some(hash) = self.force_hash {
            return hash;
        }
        composite_hash(kind, parts)
    }
    fn record_hash(&self, record: Record<'_>) -> Result<u32, i32> {
        #[cfg(test)]
        if let Some(hash) = self.force_hash {
            return Ok(hash);
        }
        record_hash(record).ok_or(ERR_INVALID)
    }
    pub(crate) fn sort_index(&mut self) {
        super::declaration_index::sort_by_key(&mut self.index, |entry| entry.order());
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
        let (record, _) =
            super::registrations::host_decode_prefix(bytes.get(offset..).ok_or(ERR_INVALID)?)?;
        let (kind, _) = key(record).ok_or(ERR_INVALID)?;
        if kind != entry.kind()
            || self.record_hash(record)? != entry.hash
            || matches!(record, Record::ResourceFontExtension(_)) != entry.is_extension()
        {
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
        self.find_parts(image, kind, &[name])
    }
    pub(crate) fn find_parts<'a>(
        &'a self,
        image: Option<&'a [u8]>,
        kind: u8,
        parts: &[&str],
    ) -> Result<Option<Record<'a>>, i32> {
        if !self.sorted {
            return Err(ERR_INVALID);
        }
        let wanted = (kind, self.hash(kind, parts));
        let start = self.index.partition_point(|entry| entry.order() < wanted);
        for entry in self.index[start..]
            .iter()
            .take_while(|entry| entry.order() == wanted)
        {
            let record = self.record(image, *entry)?;
            if matches_parts(record, kind, parts) {
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
                let a = self.record(image, *entry)?;
                let b = self.record(image, *earlier)?;
                if same_record_name(a, b) {
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
        self.visit_placed(plugin, image, &mut |record, _| visit(record))
    }
    /// [`Self::visit`], each record with its [`Place`]: `None` for one of a
    /// batch the host keeps a copy of.
    pub(crate) fn visit_placed<'a>(
        &'a self,
        plugin: &str,
        image: Option<&'a [u8]>,
        visit: &mut dyn FnMut(Record<'a>, Option<Place>),
    ) -> Result<(), i32> {
        for entry in &self.index {
            let record = self.record(image, *entry)?;
            super::declarations::validate_record(plugin, &record)?;
            visit(record, self.place(*entry));
        }
        Ok(())
    }
    fn place(&self, entry: NameEntry) -> Option<Place> {
        let batch = self.batches.get(usize::from(entry.batch))?;
        let at = u32::from(*batch.offsets.get(entry.record())?);
        Some(Place {
            turn: (u32::from(entry.batch) << 3) | u32::try_from(entry.record()).ok()?,
            at: batch.image?.offset.checked_add(at)?,
        })
    }
}
/// Where a record of a batch in the plugin's image lies: `turn` orders the
/// records as the plugin declared them (the index is in the order of their
/// names' hashes), `at` is where the record's frame starts in the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Place {
    pub(crate) turn: u32,
    pub(crate) at: u32,
}
/// A record the name index holds: exactly those `key` names, so the two never drift.
pub(crate) fn is_declaration(record: &Record<'_>) -> bool {
    key(*record).is_some()
}
fn index_size(batches: usize, names: usize) -> Result<usize, i32> {
    batches
        .checked_mul(size_of::<RawBatch>())
        .and_then(|n| {
            names
                .checked_mul(size_of::<NameEntry>())
                .and_then(|m| n.checked_add(m))
        })
        .ok_or(ERR_NO_SPACE)
}
fn growth(current: [usize; 2], needed: [usize; 2], budget: usize) -> Result<[usize; 2], i32> {
    let base = core::array::from_fn::<_, 2, _>(|i| current[i].max(needed[i]));
    let desired = core::array::from_fn::<_, 2, _>(|i| {
        if needed[i] > current[i] {
            current[i].saturating_mul(2).max(needed[i])
        } else {
            current[i]
        }
    });
    let used = index_size(base[0], base[1])?;
    let spare = budget.checked_sub(used).ok_or(ERR_NO_SPACE)?;
    let expansion = index_size(desired[0] - base[0], desired[1] - base[1])?;
    if expansion <= spare {
        return Ok(desired);
    }
    // Share the remaining budget between both vectors in the same ratio as
    // their desired growth. Rounding down keeps the reservation within budget.
    Ok(core::array::from_fn(|i| {
        base[i] + (desired[i] - base[i]) * spare / expansion
    }))
}
// Only these six kinds have individual count caps. Screen/action entries
// share the aggregate index_cost budget and use no per-kind raw vectors.
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
/// The bytes from record `index` of the batch `wire` on (0 the first): its
/// frames walked, not decoded. `ERR_INVALID` for a place past the batch's
/// count or a frame that leaves it.
pub(crate) fn record_at(wire: &[u8], index: usize) -> Result<&[u8], i32> {
    let mut cursor = 0;
    let count = varint(wire, &mut cursor)?;
    if index >= count as usize {
        return Err(ERR_INVALID);
    }
    for _ in 0..index {
        varint(wire, &mut cursor)?;
        let len = usize::try_from(varint(wire, &mut cursor)?).map_err(|_| ERR_INVALID)?;
        cursor = cursor
            .checked_add(len)
            .filter(|end| *end <= wire.len())
            .ok_or(ERR_INVALID)?;
    }
    wire.get(cursor..).ok_or(ERR_INVALID)
}
/// The LEB128 `u32` at `cursor` (moved past it); `ERR_INVALID` past the end or
/// over 32 bits.
pub(crate) fn varint(wire: &[u8], cursor: &mut usize) -> Result<u32, i32> {
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
