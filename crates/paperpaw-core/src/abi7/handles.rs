//! Bounded persistent handles for native host adapters.
//!
//! The caller supplies a host-assigned, never-reused owner identity. The wire
//! handle has ten slot bits and 22 generation bits. Generation zero is invalid.
//! Exhausted slots stay retired for the registry lifetime, including teardown.

extern crate alloc;
use alloc::vec::Vec;

const GROWTH: usize = 8;
const INDEX_BITS: u32 = 10;
const SLOTS: usize = 1 << INDEX_BITS;
const INDEX_MASK: u32 = (1 << INDEX_BITS) - 1;
const MAX_GENERATION: u32 = u32::MAX >> INDEX_BITS;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Kind {
    Font,
    Service,
    Blob,
}

const fn limit(kind: Kind) -> usize {
    match kind {
        Kind::Font => 8,
        Kind::Blob => 2,
        Kind::Service => SLOTS,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Error {
    State,
    Busy,
    Capacity,
    Allocation,
}

/// Host-only resources cancel without plugin callbacks or allocation. Values
/// must hold no plugin callback, pointer into plugin state or code, or borrowed
/// plugin buffer/image. A skipped fallback release may leave an entry under a
/// dead owner until restart; it must never deliver into that plugin. Completion
/// paths check owner lifetime and cancellation before reading any payload.
/// Release runs after the value leaves the registry borrow and must be safe
/// even if cancellation failed or never ran.
pub(crate) trait Release {
    fn cancel(&mut self) {}
    fn release(self);
}

/// Cancel under a temporary registry borrow; cancellation never calls plugins.
pub(crate) fn cancel_owner<R: Release>(
    registry: &core::cell::RefCell<Handles<R>>,
    owner: u64,
) -> Result<(), Error> {
    registry
        .try_borrow_mut()
        .map_err(|_| Error::Busy)?
        .each_owner_mut(owner, Release::cancel);
    Ok(())
}

/// Each value leaves the registry borrow before releasing its host resource.
pub(crate) fn release_owner<R: Release>(
    registry: &core::cell::RefCell<Handles<R>>,
    owner: u64,
) -> Result<(), Error> {
    loop {
        let resource = registry
            .try_borrow_mut()
            .map_err(|_| Error::Busy)?
            .take_owner(owner);
        let Some(resource) = resource else {
            return Ok(());
        };
        resource.release();
    }
}

/// Releasing an individual running operation first cancels it. Invalidate its
/// handle before cleanup, so a late completion cannot resolve it again.
pub(crate) fn release<R: Release>(
    registry: &core::cell::RefCell<Handles<R>>,
    owner: u64,
    kind: Kind,
    handle: u32,
) -> Result<(), Error> {
    let mut resource = registry
        .try_borrow_mut()
        .map_err(|_| Error::Busy)?
        .remove(owner, kind, handle)?;
    resource.cancel();
    resource.release();
    Ok(())
}

impl Error {
    pub(crate) fn status(self) -> i32 {
        match self {
            Self::State => paperpaw_native::host_glue::ERR_STATE,
            Self::Busy => paperpaw_native::host_glue::ERR_BUSY,
            Self::Capacity => paperpaw_native::host_glue::ERR_FULL,
            Self::Allocation => paperpaw_native::host_glue::ERR_NO_MEMORY,
        }
    }
}

// T follows Release's host-only lifetime rule. No leaked entry may keep a
// plugin callback or borrowed module/state/image pointer.
struct Entry<T> {
    owner: u64,
    kind: Kind,
    value: T,
}

struct Slot<T> {
    generation: u32,
    entry: Option<Entry<T>>,
}

pub(crate) struct Handles<T> {
    slots: Vec<Slot<T>>,
}

impl<T> Handles<T> {
    pub(crate) fn new() -> Self {
        Self { slots: Vec::new() }
    }

    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.slots.capacity()
    }

    /// Admission checks without allocating or consuming a resource.
    pub(crate) fn check_insert(&self, owner: u64, kind: Kind) -> Result<(), Error> {
        if owner == 0 {
            return Err(Error::State);
        }
        let owned = self
            .slots
            .iter()
            .filter(|slot| {
                slot.entry
                    .as_ref()
                    .is_some_and(|entry| entry.owner == owner && entry.kind == kind)
            })
            .count();
        if owned >= limit(kind)
            || (self.slots.len() == SLOTS
                && !self
                    .slots
                    .iter()
                    .any(|slot| slot.entry.is_none() && slot.generation != 0))
        {
            return Err(Error::Capacity);
        }
        Ok(())
    }

    /// Whether `owner` holds any handle.
    #[allow(
        dead_code,
        reason = "parked plugins (#2638); unused by tests/abi7_teardown.rs"
    )]
    pub(crate) fn holds(&self, owner: u64) -> bool {
        self.slots.iter().any(|slot| {
            slot.entry
                .as_ref()
                .is_some_and(|entry| entry.owner == owner)
        })
    }

    /// Host-only budget accounting; the visitor cannot reenter the registry.
    pub(crate) fn visit_kind(&self, kind: Kind, mut visit: impl FnMut(&T)) {
        for slot in &self.slots {
            if let Some(entry) = &slot.entry
                && entry.kind == kind
            {
                visit(&entry.value);
            }
        }
    }

    /// On refusal the value returns to its caller; no resource is lost.
    /// Service callers further restrict admission to declared, distinct keys.
    pub(crate) fn insert(&mut self, owner: u64, kind: Kind, value: T) -> Result<u32, (Error, T)> {
        self.insert_limited(owner, kind, value, limit(kind))
    }

    pub(crate) fn insert_limited(
        &mut self,
        owner: u64,
        kind: Kind,
        value: T,
        requested_limit: usize,
    ) -> Result<u32, (Error, T)> {
        if owner == 0 {
            return Err((Error::State, value));
        }
        let limit = requested_limit.min(limit(kind));
        let owned = self
            .slots
            .iter()
            .filter(|slot| {
                slot.entry
                    .as_ref()
                    .is_some_and(|entry| entry.owner == owner && entry.kind == kind)
            })
            .count();
        if owned >= limit {
            return Err((Error::Capacity, value));
        }
        let index = match self
            .slots
            .iter()
            .position(|slot| slot.entry.is_none() && slot.generation != 0)
        {
            Some(index) => index,
            None => {
                if self.slots.len() == SLOTS {
                    return Err((Error::Capacity, value));
                }
                if self.slots.len() == self.slots.capacity() {
                    let additional = GROWTH.min(SLOTS - self.slots.len());
                    if self.slots.try_reserve_exact(additional).is_err() {
                        return Err((Error::Allocation, value));
                    }
                }
                let index = self.slots.len();
                self.slots.push(Slot {
                    generation: 1,
                    entry: None,
                });
                index
            }
        };
        let slot = &mut self.slots[index];
        slot.entry = Some(Entry { owner, kind, value });
        // The slot cap fits exactly in the low ten bits.
        Ok((slot.generation << INDEX_BITS) | u32::try_from(index).unwrap_or(0))
    }

    fn slot(&self, owner: u64, kind: Kind, handle: u32) -> Result<usize, Error> {
        let index = (handle & INDEX_MASK) as usize;
        let generation = handle >> INDEX_BITS;
        let slot = self.slots.get(index).ok_or(Error::State)?;
        if generation == 0 || generation != slot.generation {
            return Err(Error::State);
        }
        match &slot.entry {
            Some(entry) if owner != 0 && entry.owner == owner && entry.kind == kind => Ok(index),
            _ => Err(Error::State),
        }
    }

    pub(crate) fn get(&self, owner: u64, kind: Kind, handle: u32) -> Result<&T, Error> {
        let index = self.slot(owner, kind, handle)?;
        self.slots[index]
            .entry
            .as_ref()
            .map(|entry| &entry.value)
            .ok_or(Error::State)
    }

    pub(crate) fn remove(&mut self, owner: u64, kind: Kind, handle: u32) -> Result<T, Error> {
        let index = self.slot(owner, kind, handle)?;
        let slot = &mut self.slots[index];
        let entry = slot.entry.take().ok_or(Error::State)?;
        advance(slot);
        Ok(entry.value)
    }

    /// Visit this owner's live resources without allocating a temporary list.
    /// The visitor must not call plugin code or reenter this registry.
    pub(crate) fn each_owner_mut(&mut self, owner: u64, mut visit: impl FnMut(&mut T)) {
        for slot in &mut self.slots {
            if let Some(entry) = &mut slot.entry
                && entry.owner == owner
            {
                visit(&mut entry.value);
            }
        }
    }

    /// Invalidate first, then let the caller release the resource outside its
    /// registry borrow. Repeating this drains an owner without a temporary Vec.
    pub(crate) fn take_owner(&mut self, owner: u64) -> Option<T> {
        let slot = self.slots.iter_mut().find(|slot| {
            slot.entry
                .as_ref()
                .is_some_and(|entry| entry.owner == owner)
        })?;
        let entry = slot.entry.take()?;
        advance(slot);
        Some(entry.value)
    }

    /// Drain one family without invalidating another family's handles.
    pub(crate) fn take_owner_kind(&mut self, owner: u64, kind: Kind) -> Option<T> {
        let slot = self.slots.iter_mut().find(|slot| {
            slot.entry
                .as_ref()
                .is_some_and(|entry| entry.owner == owner && entry.kind == kind)
        })?;
        let entry = slot.entry.take()?;
        advance(slot);
        Some(entry.value)
    }
}

fn advance<T>(slot: &mut Slot<T>) {
    slot.generation = if slot.generation == MAX_GENERATION {
        0
    } else {
        slot.generation + 1
    };
}

#[cfg(test)]
pub(crate) const fn slot_bytes<T>() -> usize {
    core::mem::size_of::<Slot<T>>()
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use super::*;

    #[test]
    fn cancellation_visits_only_the_owner_before_resources_are_taken_and_released() {
        use alloc::{rc::Rc, vec::Vec};
        use core::cell::RefCell;
        struct Resource {
            id: u8,
            events: Rc<RefCell<Vec<(u8, bool)>>>,
        }
        impl Release for Resource {
            fn cancel(&mut self) {
                self.events.borrow_mut().push((self.id, false));
            }
            fn release(self) {
                self.events.borrow_mut().push((self.id, true));
            }
        }
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut handles = Handles::new();
        for (owner, id) in [(1, 1), (2, 2), (1, 3)] {
            assert!(
                handles
                    .insert(
                        owner,
                        Kind::Blob,
                        Resource {
                            id,
                            events: Rc::clone(&events)
                        }
                    )
                    .is_ok()
            );
        }
        let registry = RefCell::new(handles);
        let borrowed = registry.borrow_mut();
        assert_eq!(cancel_owner(&registry, 1), Err(Error::Busy));
        assert_eq!(release_owner(&registry, 1), Err(Error::Busy));
        assert_eq!(release(&registry, 1, Kind::Blob, 0), Err(Error::Busy));
        assert_eq!(Error::Busy.status(), paperpaw_native::host_glue::ERR_BUSY);
        assert!(events.borrow().is_empty());
        drop(borrowed);
        cancel_owner(&registry, 1).unwrap();
        release_owner(&registry, 1).unwrap();
        assert_eq!(
            &*events.borrow(),
            &[(1, false), (3, false), (1, true), (3, true)]
        );
        registry
            .borrow_mut()
            .each_owner_mut(1, |_| panic!("removed owner"));
        let handle = registry
            .borrow_mut()
            .insert(
                2,
                Kind::Blob,
                Resource {
                    id: 4,
                    events: Rc::clone(&events),
                },
            )
            .ok()
            .unwrap();
        release(&registry, 2, Kind::Blob, handle).unwrap();
        assert_eq!(
            release(&registry, 2, Kind::Blob, handle)
                .unwrap_err()
                .status(),
            paperpaw_native::host_glue::ERR_STATE
        );
        assert!(registry.borrow_mut().take_owner(2).is_some());
    }

    #[test]
    fn repeated_acquire_release_and_teardown_keep_slot_memory_flat() {
        let mut handles = Handles::new();
        let first = handles.insert(1, Kind::Font, 7).unwrap();
        let capacity = handles.capacity();
        let retained_bytes = capacity * slot_bytes::<i32>();
        assert_eq!(capacity, GROWTH);
        assert_eq!(handles.remove(1, Kind::Font, first), Ok(7));
        let mut previous = first;
        for owner in 2..10_002 {
            let handle = handles.insert(owner, Kind::Font, 8).unwrap();
            assert_ne!(handle, previous);
            assert_eq!(handle & INDEX_MASK, first & INDEX_MASK);
            assert_eq!(handles.get(owner, Kind::Font, previous), Err(Error::State));
            assert_eq!(handles.take_owner(owner), Some(8));
            assert!(handles.take_owner(owner).is_none());
            assert_eq!(handles.slots.len(), 1);
            assert_eq!(handles.capacity() * slot_bytes::<i32>(), retained_bytes);
            previous = handle;
        }
    }

    #[test]
    fn generation_exhaustion_retires_the_slot_even_after_owner_teardown() {
        let mut handles = Handles::new();
        handles.slots.push(Slot {
            generation: MAX_GENERATION,
            entry: None,
        });
        let old = handles.insert(1, Kind::Font, 7).unwrap();
        assert_eq!(handles.take_owner(1), Some(7));
        let new = handles.insert(2, Kind::Font, 8).unwrap();
        assert_ne!(new & INDEX_MASK, old & INDEX_MASK);
        assert_eq!(handles.get(1, Kind::Font, old), Err(Error::State));
        assert_eq!(handles.slots[0].generation, 0);
    }
    #[test]
    fn family_expiry_preserves_fonts_and_other_owners_and_invalidates_blobs() {
        let mut handles = Handles::new();
        assert_eq!(handles.check_insert(0, Kind::Blob), Err(Error::State));
        let font = handles.insert(1, Kind::Font, 7).unwrap();
        let blob = handles.insert(1, Kind::Blob, 11).unwrap();
        handles.insert(1, Kind::Blob, 13).unwrap();
        let other = handles.insert(2, Kind::Blob, 17).unwrap();
        assert_eq!(handles.check_insert(1, Kind::Blob), Err(Error::Capacity));
        let mut sum = 0;
        handles.visit_kind(Kind::Blob, |value| sum += value);
        assert_eq!(sum, 41);
        assert_eq!(handles.take_owner_kind(1, Kind::Blob), Some(11));
        assert_eq!(handles.get(1, Kind::Blob, blob), Err(Error::State));
        assert_eq!(handles.check_insert(1, Kind::Blob), Ok(()));
        assert_eq!(handles.take_owner_kind(1, Kind::Blob), Some(13));
        assert_eq!(handles.take_owner_kind(1, Kind::Blob), None);
        assert_eq!(handles.get(1, Kind::Font, font), Ok(&7));
        assert_eq!(handles.get(2, Kind::Blob, other), Ok(&17));
    }
}
