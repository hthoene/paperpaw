//! Borrowed declaration storage pending the draft-4 SDK registry boundary.
//!
//! Only committed construction batches may enter this store. Registry adapters
//! will add cross-plugin reference checks before prepare. No decoded view lives
//! beyond its visitor, and the retained owner supplies every image lookup.

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
    Committed(super::raw_declarations::RawBatch),
}
impl Input<'_> {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Borrowed(bytes) => bytes,
            Self::Committed(raw) => &raw.bytes,
        }
    }
    fn address(&self) -> usize {
        match self {
            Self::Borrowed(bytes) => bytes.as_ptr() as usize,
            Self::Committed(raw) => raw.submitted,
        }
    }
    fn own(self, allocations: &mut Allocations) -> Result<DeclRef, i32> {
        match self {
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
        self.stage(Input::Committed(raw), allocations)
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
        if let Input::Committed(raw) = &input
            && (raw.count != decoded.len()
                || raw
                    .offsets
                    .iter()
                    .take(raw.count)
                    .any(|offset| usize::from(*offset) >= bytes.len()))
        {
            return Err(ERR_INVALID);
        }
        let mut counts = [0usize; 6];
        let mut kinds = [None; super::registrations::REGISTRATION_MAX_RECORDS];
        for (index, record) in decoded.iter().enumerate() {
            kinds[index] = Kind::of(&record);
            if let Some(kind) = kinds[index] {
                counts[kind as usize] += 1;
            }
        }
        if counts.iter().all(|&n| n == 0) {
            return Ok(());
        }
        let batch = u16::try_from(self.batches.len()).map_err(|_| ERR_NO_SPACE)?;
        let mut reference = None;
        self.source.with_image(&mut |image| {
            reference = image.and_then(|image| image_ref_at(image, input.address(), bytes));
        });
        let owned = if reference.is_none() { bytes.len() } else { 0 };
        let cost = size_of::<DeclRef>() + counts.iter().sum::<usize>() * size_of::<Slot>() + owned;
        let resident = self
            .resident
            .checked_add(cost)
            .filter(|n| *n <= DECLARATIONS_MAX_BYTES)
            .ok_or(ERR_NO_SPACE)?;
        let reference = match reference {
            Some(reference) => reference,
            None => input.own(allocations)?,
        };
        allocations.reserve(&mut self.batches, 1)?;
        for (registry, count) in self.kinds.iter_mut().zip(counts) {
            allocations.reserve(registry, count)?;
        }
        for (record, kind) in kinds.into_iter().enumerate() {
            if let Some(kind) = kind {
                // The decoder bounds each batch to eight records.
                self.kinds[kind as usize].push(Slot {
                    batch,
                    record: u16::try_from(record).unwrap_or(u16::MAX),
                });
            }
        }
        self.batches.push(reference);
        self.resident = resident;
        Ok(())
    }
}

fn image_ref(image: &[u8], bytes: &[u8]) -> Option<DeclRef> {
    image_ref_at(image, bytes.as_ptr() as usize, bytes)
}
fn image_ref_at(image: &[u8], submitted: usize, bytes: &[u8]) -> Option<DeclRef> {
    let offset = submitted.checked_sub(image.as_ptr() as usize)?;
    let end = offset.checked_add(bytes.len())?;
    if image.get(offset..end)? != bytes {
        return None;
    }
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
        let Some(reference) = owner.batches.get(usize::from(key.batch)) else {
            return false;
        };
        let mut found = false;
        // Even Owned batches require a live owner. The bridge returns None after
        // unloading, and holds the module guard throughout decode and the visit.
        owner.source.with_image(&mut |image| {
            let Some(image) = image else {
                return;
            };
            let bytes = match reference {
                DeclRef::Image { offset, len } => {
                    usize::try_from(*offset).ok().and_then(|offset| {
                        offset
                            .checked_add(*len as usize)
                            .and_then(|end| image.get(offset..end))
                    })
                }
                DeclRef::Owned(bytes) => Some(bytes.as_ref()),
            };
            let result = bytes.ok_or(ERR_INVALID).and_then(|bytes| {
                let batch = records_decode(bytes)?;
                declarations::validate_batch(owner.id.as_str(), &batch)?;
                let record = batch
                    .iter()
                    .nth(usize::from(key.record))
                    .ok_or(ERR_INVALID)?;
                // Kind changes after admission must not expose a different kind
                // through an index prepared for the original declaration.
                let kind = Kind::of(&record).ok_or(ERR_INVALID)?;
                if !owner.kinds[kind as usize]
                    .iter()
                    .any(|slot| slot.batch == key.batch && slot.record == key.record)
                {
                    return Err(ERR_INVALID);
                }
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

#[cfg(test)]
#[path = "declaration_store_tests.rs"]
mod tests;

// The device check validates layout instead of extrapolating host enum sizes.
#[cfg(target_pointer_width = "32")]
const _: () = {
    assert!(size_of::<DeclRef>() == 12);
    assert!(size_of::<DeclKey>() == 16);
    assert!(size_of::<Slot>() == 4);
};
