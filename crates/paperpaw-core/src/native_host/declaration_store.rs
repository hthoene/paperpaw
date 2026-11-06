//! Borrowed declaration storage pending the draft-4 SDK registry boundary.
//!
//! Only committed construction batches may enter this store. Registry adapters
//! will add cross-plugin reference checks before prepare. No decoded view lives
//! beyond its visitor, and the retained owner supplies every image lookup.
//! Consumers must not rely on uniqueness or valid references after admission:
//! native plugins can change image bytes without a sandbox.

use super::{
    declarations,
    registrations::{Record, records_decode},
    staged::DECLARATIONS_MAX_BYTES,
};
use alloc::{boxed::Box, rc::Rc, vec::Vec};
use core::mem::size_of;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_STATE};
use paperpaw_sdk::Id;

/// The bridge holds the module guard and excludes image release during a visit.
/// Methods must not invoke plugin callbacks or reenter the store.
pub(crate) trait ImageOwner {
    fn generation(&self) -> u32;
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>));
    fn invalid_declaration(&self, error: i32);
}

#[derive(Debug)]
pub(crate) enum DeclRef {
    Image { offset: u32, len: u32 },
    Owned(Box<[u8]>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DeclKey {
    owner: u16,
    batch: u16,
    record: u16,
    generation: u32,
    incarnation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Group,
    Spec,
    Extra,
    Hyphenation,
    Face,
    Extension,
}
impl Kind {
    fn of(record: &Record<'_>) -> Option<Self> {
        Some(match record {
            Record::SettingGroup(_) => Self::Group,
            Record::SettingSpec(_) => Self::Spec,
            Record::SettingExtra(_) => Self::Extra,
            Record::ResourceHyphenation(_) => Self::Hyphenation,
            Record::ResourceFontFace(_) => Self::Face,
            Record::ResourceFontExtension(_) => Self::Extension,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy)]
struct Slot {
    hash: u32,
    batch: u16,
    record: u16,
}

/// Fault injection is local to a transaction, so parallel tests cannot interfere.
#[derive(Default)]
pub(crate) struct Allocations {
    #[cfg(test)]
    pub(crate) fail_after: Option<usize>,
}
impl Allocations {
    fn reserve<T>(&mut self, target: &mut Vec<T>, count: usize) -> Result<(), i32> {
        if count == 0 {
            return Ok(());
        }
        #[cfg(test)]
        if let Some(left) = &mut self.fail_after {
            if *left == 0 {
                return Err(ERR_NO_MEMORY);
            }
            *left -= 1;
        }
        target.try_reserve_exact(count).map_err(|_| ERR_NO_MEMORY)
    }
}

enum Input<'a> {
    Borrowed(&'a [u8]),
    Image {
        bytes: &'a [u8],
        offset: u32,
        len: u32,
        entries: u16,
    },
    Committed(super::raw_declarations::RawBatch),
}
impl Input<'_> {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Borrowed(bytes) | Self::Image { bytes, .. } => bytes,
            Self::Committed(raw) => &raw.bytes,
        }
    }
    fn own(self, allocations: &mut Allocations) -> Result<DeclRef, i32> {
        match self {
            Self::Image { .. } => Err(ERR_STATE),
            Self::Committed(raw) => Ok(DeclRef::Owned(raw.bytes)),
            Self::Borrowed(bytes) => {
                let mut copy = Vec::new();
                allocations.reserve(&mut copy, bytes.len())?;
                copy.extend_from_slice(bytes);
                Ok(DeclRef::Owned(copy.into_boxed_slice()))
            }
        }
    }
}

pub(crate) struct PendingDeclarations {
    id: Id,
    generation: u32,
    source: Rc<dyn ImageOwner>,
    batches: Vec<DeclRef>,
    kinds: [Vec<Slot>; 6],
    resident: usize,
    incarnation: u32,
    failure: Option<i32>,
    index_bytes: usize,
    image_entries: usize,
    owned_counts: [usize; 6],
}
impl PendingDeclarations {
    pub(crate) fn new(id: Id, source: Rc<dyn ImageOwner>) -> Self {
        Self {
            id,
            generation: source.generation(),
            source,
            batches: Vec::new(),
            kinds: core::array::from_fn(|_| Vec::new()),
            resident: 0,
            incarnation: 0,
            failure: None,
            index_bytes: 0,
            image_entries: 0,
            owned_counts: [0; 6],
        }
    }

    /// Stage one whole batch. Failed reservations may grow capacity but never
    /// publish a batch or an index. Input bytes are copied before this returns.
    pub(crate) fn stage_batch(
        &mut self,
        bytes: &[u8],
        allocations: &mut Allocations,
    ) -> Result<(), i32> {
        self.stage(Input::Borrowed(bytes), allocations)
    }

    pub(crate) fn stage_committed(
        &mut self,
        raw: super::raw_declarations::RawBatch,
        allocations: &mut Allocations,
    ) -> Result<(), i32> {
        if raw.image.is_none() {
            return self.stage(Input::Committed(raw), allocations);
        }
        let range = raw.image.ok_or(ERR_STATE)?;
        let source = Rc::clone(&self.source);
        let mut result = Err(ERR_STATE);
        source.with_image(&mut |image| {
            result = raw.view(image).and_then(|bytes| {
                self.stage(
                    Input::Image {
                        bytes,
                        offset: range.offset,
                        len: range.len,
                        entries: raw.entries,
                    },
                    allocations,
                )
            });
        });
        if let Err(error) = result {
            self.failure = Some(error);
        }
        result
    }

    fn stage(&mut self, input: Input<'_>, allocations: &mut Allocations) -> Result<(), i32> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let result = self.stage_inner(input, allocations);
        if let Err(error) = result {
            self.failure = Some(error);
        }
        result
    }

    fn stage_inner(&mut self, input: Input<'_>, allocations: &mut Allocations) -> Result<(), i32> {
        if self.source.generation() != self.generation {
            return Err(ERR_STATE);
        }
        let bytes = input.bytes();
        let decoded = records_decode(bytes)?;
        declarations::validate_batch(self.id.as_str(), &decoded)?;
        let offsets = super::raw_declarations::offsets(bytes, decoded.len())?;
        let mut counts = [0usize; 6];
        let mut kinds = [None; super::registrations::REGISTRATION_MAX_RECORDS];
        for (index, record) in decoded.iter().enumerate() {
            if let Some(kind) = Kind::of(&record) {
                let (category, name) = super::declaration_index::key(record).ok_or(ERR_INVALID)?;
                kinds[index] = Some((kind, super::declaration_index::name_hash(category, name)));
                counts[kind as usize] += 1;
            }
        }
        let expected = match &input {
            Input::Committed(raw) => Some(raw.entries),
            Input::Image { entries, .. } => Some(*entries),
            Input::Borrowed(_) => None,
        };
        if expected.is_some_and(|expected| usize::from(expected) != counts.iter().sum::<usize>()) {
            return Err(ERR_INVALID);
        }
        if counts.iter().all(|&n| n == 0) {
            return Ok(());
        }
        let batch = u16::try_from(self.batches.len()).map_err(|_| ERR_NO_SPACE)?;
        let mut reference = match &input {
            Input::Image { offset, len, .. } => Some(DeclRef::Image {
                offset: *offset,
                len: *len,
            }),
            _ => None,
        };
        if matches!(input, Input::Borrowed(_)) {
            self.source.with_image(&mut |image| {
                reference = image.and_then(|image| image_ref(image, bytes));
            });
        }
        let image = reference.is_some();
        let (resident, index_bytes, image_entries) = self.charge(counts, image, bytes.len())?;
        let reference = match reference {
            Some(reference) => reference,
            None => input.own(allocations)?,
        };
        allocations.reserve(&mut self.batches, 1)?;
        for (registry, count) in self.kinds.iter_mut().zip(counts) {
            allocations.reserve(registry, count)?;
        }
        for (record, kind) in kinds.into_iter().enumerate() {
            if let Some((kind, hash)) = kind {
                // The decoder bounds each batch to eight records.
                self.kinds[kind as usize].push(Slot {
                    hash,
                    batch,
                    record: offsets[record],
                });
            }
        }
        self.batches.push(reference);
        self.resident = resident;
        self.index_bytes = index_bytes;
        self.image_entries = image_entries;
        if !image {
            for (total, count) in self.owned_counts.iter_mut().zip(counts) {
                *total += count;
            }
        }
        Ok(())
    }

    fn charge(
        &self,
        counts: [usize; 6],
        image: bool,
        bytes: usize,
    ) -> Result<(usize, usize, usize), i32> {
        use super::staged::{
            EXTRAS_MAX, FONT_EXTENSIONS_MAX, FONT_FACES_MAX, GROUPS_MAX, HYPHENATIONS_MAX,
            IMAGE_DECLARATIONS_MAX, IMAGE_INDEX_MAX_BYTES, SPECS_MAX,
        };
        let entries = counts.iter().sum::<usize>();
        let index = size_of::<DeclRef>() + entries * size_of::<Slot>();
        if image {
            let image_entries = self
                .image_entries
                .checked_add(entries)
                .filter(|n| *n <= IMAGE_DECLARATIONS_MAX)
                .ok_or(ERR_NO_SPACE)?;
            let index_bytes = self
                .index_bytes
                .checked_add(index)
                .filter(|n| *n <= IMAGE_INDEX_MAX_BYTES)
                .ok_or(ERR_NO_SPACE)?;
            Ok((self.resident, index_bytes, image_entries))
        } else {
            let limits = [
                GROUPS_MAX,
                SPECS_MAX,
                EXTRAS_MAX,
                HYPHENATIONS_MAX,
                FONT_FACES_MAX,
                FONT_EXTENSIONS_MAX,
            ];
            if self
                .owned_counts
                .iter()
                .zip(counts)
                .zip(limits)
                .any(|((&old, new), limit)| old + new > limit)
            {
                return Err(ERR_NO_SPACE);
            }
            let resident = self
                .resident
                .checked_add(index)
                .and_then(|n| n.checked_add(bytes))
                .filter(|n| *n <= DECLARATIONS_MAX_BYTES)
                .ok_or(ERR_NO_SPACE)?;
            Ok((resident, self.index_bytes, self.image_entries))
        }
    }
}

fn image_ref(image: &[u8], bytes: &[u8]) -> Option<DeclRef> {
    let offset = (bytes.as_ptr() as usize).checked_sub(image.as_ptr() as usize)?;
    image.get(offset..offset.checked_add(bytes.len())?)?;
    Some(DeclRef::Image {
        offset: u32::try_from(offset).ok()?,
        len: u32::try_from(bytes.len()).ok()?,
    })
}

#[derive(Default)]
pub(crate) struct DeclarationStore {
    owners: Vec<Option<PendingDeclarations>>,
    incarnation: u32,
}

/// Exclusive preparation prevents a second transaction invalidating reservations.
pub(crate) struct Prepared<'a> {
    store: &'a mut DeclarationStore,
    pending: PendingDeclarations,
    at: usize,
}
impl Prepared<'_> {
    /// Every allocation and validation completed before this point.
    pub(crate) fn commit(self) {
        self.store.incarnation = self.pending.incarnation;
        if self.at == self.store.owners.len() {
            self.store.owners.push(Some(self.pending));
        } else {
            self.store.owners[self.at] = Some(self.pending);
        }
    }
}
impl DeclarationStore {
    pub(crate) fn prepare<'a>(
        &'a mut self,
        mut pending: PendingDeclarations,
        allocations: &mut Allocations,
    ) -> Result<Prepared<'a>, i32> {
        if let Some(error) = pending.failure {
            return Err(error);
        }
        if pending.source.generation() != pending.generation {
            return Err(ERR_STATE);
        }
        for entries in &mut pending.kinds {
            entries.sort_unstable_by_key(|entry| entry.hash);
        }
        let at = self
            .owners
            .iter()
            .position(|owner| owner.as_ref().is_some_and(|old| old.id == pending.id));
        let at = if let Some(at) = at {
            let old = self.owners[at].as_ref().ok_or(ERR_STATE)?;
            if pending.generation <= old.generation {
                return Err(ERR_STATE);
            }
            at
        } else if let Some(at) = self.owners.iter().position(Option::is_none) {
            at
        } else {
            u16::try_from(self.owners.len()).map_err(|_| ERR_NO_SPACE)?;
            allocations.reserve(&mut self.owners, 1)?;
            self.owners.len()
        };
        pending.incarnation = self.incarnation.checked_add(1).ok_or(ERR_NO_SPACE)?;
        Ok(Prepared {
            store: self,
            pending,
            at,
        })
    }

    /// Remove indexes before releasing the retained owner/module guard.
    pub(crate) fn remove_owner(&mut self, id: &Id, generation: u32) {
        for owner in &mut self.owners {
            if owner
                .as_ref()
                .is_some_and(|owner| owner.id == *id && owner.generation == generation)
            {
                *owner = None;
            }
        }
    }

    pub(crate) fn keys(&self, kind: Kind, visit: &mut dyn FnMut(DeclKey)) {
        for (index, owner) in self.owners.iter().enumerate() {
            let Some(owner) = owner else {
                continue;
            };
            if owner.source.generation() != owner.generation {
                continue;
            }
            for slot in &owner.kinds[kind as usize] {
                visit(DeclKey {
                    owner: u16::try_from(index).unwrap_or(u16::MAX),
                    batch: slot.batch,
                    record: slot.record,
                    generation: owner.generation,
                    incarnation: owner.incarnation,
                });
            }
        }
    }

    /// Binary name lookup shares the resident sorted slots with key visitors.
    pub(crate) fn find(
        &self,
        plugin: &str,
        kind: Kind,
        name: &str,
        visit: &mut dyn for<'a> FnMut(Record<'a>),
    ) -> bool {
        let Some((index, owner)) = self.owners.iter().enumerate().find_map(|(index, owner)| {
            owner
                .as_ref()
                .filter(|owner| owner.id.as_str() == plugin)
                .map(|owner| (index, owner))
        }) else {
            return false;
        };
        let category = if kind == Kind::Extension {
            4
        } else {
            kind as u8
        };
        let hash = super::declaration_index::name_hash(category, name);
        let slots = &owner.kinds[kind as usize];
        let from = slots.partition_point(|slot| slot.hash < hash);
        for slot in slots[from..].iter().take_while(|slot| slot.hash == hash) {
            let key = DeclKey {
                owner: u16::try_from(index).unwrap_or(u16::MAX),
                batch: slot.batch,
                record: slot.record,
                generation: owner.generation,
                incarnation: owner.incarnation,
            };
            let mut found = false;
            self.with_record(key, &mut |record| {
                if super::declaration_index::key(record).is_some_and(|(_, actual)| {
                    super::declaration_index::same_name(category, actual, name)
                }) {
                    found = true;
                    visit(record);
                }
            });
            if found {
                return true;
            }
        }
        false
    }

    /// The higher-ranked visitor cannot retain a reference into the module.
    pub(crate) fn with_record(
        &self,
        key: DeclKey,
        visit: &mut dyn for<'a> FnMut(Record<'a>),
    ) -> bool {
        let Some(owner) = self
            .owners
            .get(usize::from(key.owner))
            .and_then(Option::as_ref)
        else {
            return false;
        };
        if owner.incarnation != key.incarnation
            || owner.generation != key.generation
            || owner.source.generation() != key.generation
        {
            return false;
        }
        let mut found = false;
        // Even Owned batches require a live owner. The bridge returns None after
        // unloading, and holds the module guard throughout decode and the visit.
        owner.source.with_image(&mut |image| {
            let Some(image) = image else {
                return;
            };
            let result = decode_entry(owner, image, key.batch, key.record).and_then(|record| {
                check_indexed_name(owner, image, key, record)?;
                visit(record);
                Ok(())
            });
            match result {
                Ok(()) => found = true,
                Err(error) => owner.source.invalid_declaration(error),
            }
        });
        found
    }
}

fn decode_entry<'a>(
    owner: &'a PendingDeclarations,
    image: &'a [u8],
    batch: u16,
    record: u16,
) -> Result<Record<'a>, i32> {
    let reference = owner.batches.get(usize::from(batch)).ok_or(ERR_INVALID)?;
    let bytes = match reference {
        DeclRef::Image { offset, len } => {
            let start = usize::try_from(*offset).map_err(|_| ERR_INVALID)?;
            start
                .checked_add(*len as usize)
                .and_then(|end| image.get(start..end))
                .ok_or(ERR_INVALID)?
        }
        DeclRef::Owned(bytes) => bytes,
    };
    let (record, _) = super::registrations::record_decode_prefix(
        bytes.get(usize::from(record)..).ok_or(ERR_INVALID)?,
    )?;
    declarations::validate_record(owner.id.as_str(), &record)?;
    Ok(record)
}

fn check_indexed_name(
    owner: &PendingDeclarations,
    image: &[u8],
    key: DeclKey,
    record: Record<'_>,
) -> Result<(), i32> {
    use super::declaration_index::{key as name_key, name_hash, same_name};
    let kind = Kind::of(&record).ok_or(ERR_INVALID)?;
    let (category, name) = name_key(record).ok_or(ERR_INVALID)?;
    let hash = name_hash(category, name);
    let slots = &owner.kinds[kind as usize];
    let from = slots.partition_point(|slot| slot.hash < hash);
    let run = slots[from..].iter().take_while(|slot| slot.hash == hash);
    if !run
        .clone()
        .any(|slot| slot.batch == key.batch && slot.record == key.record)
    {
        return Err(ERR_INVALID);
    }
    // Revalidate collisions under the same guard. Valid changed bytes must
    // not turn distinct admitted names into duplicate keys at lookup time.
    for peer in run.filter(|slot| slot.batch != key.batch || slot.record != key.record) {
        let Ok(other) = decode_entry(owner, image, peer.batch, peer.record) else {
            continue;
        };
        if name_key(other).is_some_and(|(other_kind, other_name)| {
            other_kind == category
                && name_hash(category, other_name) == hash
                && same_name(category, name, other_name)
        }) {
            return Err(ERR_INVALID);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "declaration_store_tests.rs"]
mod tests;

// The device check validates layout instead of extrapolating host enum sizes.
#[cfg(target_pointer_width = "32")]
const _: () = {
    assert!(size_of::<DeclRef>() == 12);
    assert!(size_of::<DeclKey>() == 16);
    assert!(size_of::<Slot>() == 8);
};
