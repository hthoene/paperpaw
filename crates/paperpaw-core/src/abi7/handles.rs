//! Bounded persistent handles for the M1 host adapters.
//!
//! The caller supplies a host-assigned, never-reused owner identity. The wire
//! handle has five slot bits and 27 generation bits. Generation zero is invalid.
//! Exhausted slots stay retired for the registry lifetime, including teardown.

const SLOTS: usize = 32;
const PER_OWNER: usize = 4;
const MAX_GENERATION: u32 = u32::MAX >> 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Kind {
    Font,
    Service,
    Blob,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Error {
    State,
    Capacity,
}

/// Host-only resources cancel without plugin callbacks or allocation. Values
/// must hold no plugin callback, pointer into plugin state or code, or borrowed
/// plugin buffer/image. A skipped fallback release may leave an entry under a
/// dead owner until restart; it must never deliver into that plugin. Completion
/// paths check owner lifetime and cancellation before reading any payload.
/// Release runs after the value leaves the registry borrow.
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
        .map_err(|_| Error::State)?
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
            .map_err(|_| Error::State)?
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
        .map_err(|_| Error::State)?
        .remove(owner, kind, handle)?;
    resource.cancel();
    resource.release();
    Ok(())
}

impl Error {
    pub(crate) fn status(self) -> i32 {
        match self {
            Self::State => paperpaw_native::host_glue::ERR_STATE,
            Self::Capacity => paperpaw_native::host_glue::ERR_FULL,
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
    slots: [Slot<T>; SLOTS],
}

impl<T> Handles<T> {
    pub(crate) fn new() -> Self {
        Self {
            slots: core::array::from_fn(|_| Slot {
                generation: 1,
                entry: None,
            }),
        }
    }

    /// On refusal the value returns to its caller; no resource is lost.
    pub(crate) fn insert(&mut self, owner: u64, kind: Kind, value: T) -> Result<u32, (Error, T)> {
        if owner == 0 {
            return Err((Error::State, value));
        }
        let owned = self
            .slots
            .iter()
            .filter(|slot| {
                slot.entry
                    .as_ref()
                    .is_some_and(|entry| entry.owner == owner)
            })
            .count();
        if owned >= PER_OWNER {
            return Err((Error::Capacity, value));
        }
        let Some((index, slot)) = self
            .slots
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| slot.entry.is_none() && slot.generation != 0)
        else {
            return Err((Error::Capacity, value));
        };
        slot.entry = Some(Entry { owner, kind, value });
        // There are exactly 32 slots; this index always fits the low five bits.
        Ok((slot.generation << 5) | u32::try_from(index).unwrap_or(0))
    }

    fn slot(&self, owner: u64, kind: Kind, handle: u32) -> Result<usize, Error> {
        let index = (handle & 31) as usize;
        let generation = handle >> 5;
        let slot = &self.slots[index];
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
}

fn advance<T>(slot: &mut Slot<T>) {
    slot.generation = if slot.generation == MAX_GENERATION {
        0
    } else {
        slot.generation + 1
    };
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
    fn generation_exhaustion_retires_the_slot_even_after_owner_teardown() {
        let mut handles = Handles::new();
        handles.slots[0].generation = MAX_GENERATION;
        let old = handles.insert(1, Kind::Font, 7).unwrap();
        assert_eq!(handles.take_owner(1), Some(7));
        let new = handles.insert(2, Kind::Font, 8).unwrap();
        assert_ne!(new & 31, old & 31);
        assert_eq!(handles.get(1, Kind::Font, old), Err(Error::State));
        assert_eq!(handles.slots[0].generation, 0);
    }
}
