//! Where the settings' specs are kept ([`Specs`]).

use alloc::vec::Vec;

use super::{SettingError, SettingSpec};

/// The settings' specs in registration order, found by id through an
/// index sorted by it: each id kept once. Specs kept in the program
/// (a `static` list of `SettingSpec`s, e.g. in flash) are held as the lists a plugin
/// gave ([`Registrar::add_settings`](crate::Registrar::add_settings)),
/// a reference to each list rather than to each spec; the others as they
/// are.
#[derive(Debug, Default)]
pub(super) struct Specs {
    owned: Vec<SettingSpec>,
    /// The lists of fixed specs, each with the place of its first spec:
    /// the places of fixed specs count on through the lists.
    lists: Vec<(&'static [SettingSpec], u16)>,
    /// Every spec in registration order: a place in `owned`, or among
    /// the fixed specs with [`FIXED`] set.
    order: Vec<u16>,
    /// The same places, sorted by the specs' ids.
    by_id: Vec<u16>,
}

/// Marks a place among the fixed specs.
const FIXED: u16 = 1 << 15;
/// Specs of each kind (made at run time, kept in the program) a place of
/// 15 bits holds: plenty, and two bytes a place on small devices.
const PLACES: usize = FIXED as usize;

impl Specs {
    fn at(&self, place: u16) -> &SettingSpec {
        if place & FIXED == 0 {
            return &self.owned[usize::from(place)];
        }
        let place = place & !FIXED;
        let list = self.lists.partition_point(|(_, first)| *first <= place) - 1;
        let (specs, first) = self.lists[list];
        &specs[usize::from(place - first)]
    }

    /// Where `id` is in `by_id`, or would go.
    fn find(&self, id: &str) -> Result<usize, usize> {
        self.by_id
            .binary_search_by(|&at| self.at(at).id.as_str().cmp(id))
    }

    pub(super) fn get(&self, id: &str) -> Option<&SettingSpec> {
        let at = self.find(id).ok()?;
        Some(self.at(self.by_id[at]))
    }

    /// The spec `id` to change: a fixed one is copied first, which needs
    /// a place of its own.
    pub(super) fn get_mut(&mut self, id: &str) -> Result<&mut SettingSpec, SettingError> {
        let at = self.find(id).map_err(|_| SettingError::Unknown)?;
        let place = self.by_id[at];
        let place = if place & FIXED == 0 {
            place
        } else if self.owned.len() >= PLACES {
            return Err(SettingError::Full);
        } else {
            // Room for exactly this copy: the list is kept exactly sized
            // (`try_reserve`), and a push would double it, briefly holding
            // twice the made specs (17.8 → 35.6 KiB with a linked plugin's
            // widget settings, #248). No room is the same refusal as no place.
            self.owned
                .try_reserve_exact(1)
                .map_err(|_| SettingError::Full)?;
            let copy = self.at(place).clone();
            let owned = self.owned.len() as u16;
            self.owned.push(copy);
            self.by_id[at] = owned;
            if let Some(o) = self.order.iter_mut().find(|o| **o == place) {
                *o = owned;
            }
            owned
        };
        Ok(&mut self.owned[usize::from(place)])
    }

    /// Adds `place`, whose spec's id is not there yet.
    fn index(&mut self, id: &str, place: u16) {
        let Err(at) = self.find(id) else {
            return;
        };
        self.by_id.insert(at, place);
        self.order.push(place);
    }

    /// Adds `spec`, whose id is not there yet; `false` when no place is
    /// left.
    pub(super) fn insert(&mut self, spec: SettingSpec) -> bool {
        // The place in the index is found before the spec moves in, so that
        // its id need not be copied.
        let Err(at) = self.find(spec.id.as_str()) else {
            return true;
        };
        if self.owned.len() >= PLACES {
            return false;
        }
        let place = self.owned.len() as u16;
        self.owned.push(spec);
        self.by_id.insert(at, place);
        self.order.push(place);
        true
    }

    /// Adds spec `at` of the fixed `list`, whose id is not there yet;
    /// `false` when no place is left. The specs of a list are added one
    /// after another, so the list is kept once.
    pub(super) fn insert_fixed(&mut self, list: &'static [SettingSpec], at: usize) -> bool {
        let Some(spec) = list.get(at) else {
            return true;
        };
        if self.find(spec.id.as_str()).is_ok() {
            return true;
        }
        let first = match self.lists.last() {
            Some((last, first)) if core::ptr::eq(*last, list) => *first,
            last => {
                let next = last.map_or(0, |(specs, first)| usize::from(*first) + specs.len());
                if next + list.len() > PLACES {
                    return false;
                }
                self.lists.push((list, next as u16));
                next as u16
            }
        };
        self.index(spec.id.as_str(), (first + at as u16) | FIXED);
        true
    }

    /// Whether `additional` more made specs find a place.
    pub(super) fn has_room_for(&self, additional: usize) -> bool {
        self.owned
            .len()
            .checked_add(additional)
            .is_some_and(|total| total <= PLACES)
    }

    /// Makes room for `additional` more made specs and `fixed` more kept in
    /// the program (which take a place in the two indexes only); nothing
    /// changes when the heap refuses.
    pub(super) fn try_reserve(
        &mut self,
        additional: usize,
        fixed: usize,
    ) -> Result<(), alloc::collections::TryReserveError> {
        // Exactly: a plugin's settings come in one batch, and room reserved by
        // doubling would stay resident (it was 10 KiB on the X4 profile).
        let indexed = additional.saturating_add(fixed);
        self.owned.try_reserve_exact(additional)?;
        self.order.try_reserve_exact(indexed)?;
        self.by_id.try_reserve_exact(indexed)
    }

    /// Forgets every spec whose id belongs to `plugin`; the places of the
    /// others move down over the gaps. Allocates nothing.
    pub(super) fn remove_plugin(&mut self, plugin: &str) {
        let Self {
            owned,
            lists,
            order,
            by_id,
        } = self;
        let belongs = |owned: &[SettingSpec], place: u16| {
            let spec = if place & FIXED == 0 {
                &owned[usize::from(place)]
            } else {
                let place = place & !FIXED;
                let list = lists.partition_point(|(_, first)| *first <= place) - 1;
                let (specs, first) = lists[list];
                &specs[usize::from(place - first)]
            };
            spec.id.namespace() == plugin
        };
        order.retain(|&place| !belongs(owned, place));
        by_id.retain(|&place| !belongs(owned, place));
        // A made spec's place drops by the gaps before it.
        for place in order.iter_mut().chain(by_id.iter_mut()) {
            if *place & FIXED == 0 {
                let gaps = owned[..usize::from(*place)]
                    .iter()
                    .filter(|spec| spec.id.namespace() == plugin)
                    .count();
                *place -= gaps as u16;
            }
        }
        owned.retain(|spec| spec.id.namespace() != plugin);
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &SettingSpec> {
        self.order.iter().map(|&place| self.at(place))
    }

    pub(super) fn shrink_to_fit(&mut self) {
        self.owned.shrink_to_fit();
        self.lists.shrink_to_fit();
        self.order.shrink_to_fit();
        self.by_id.shrink_to_fit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::groups::READING;

    static KEPT: [SettingSpec; 1] = [SettingSpec::switch("t.kept", "t.kept", true, READING)];

    #[test]
    fn changing_a_kept_spec_grows_the_made_list_by_one_not_by_doubling() {
        let mut specs = Specs::default();
        specs.try_reserve(3, 1).unwrap();
        for id in ["t.a", "t.b", "t.c"] {
            assert!(specs.insert(SettingSpec::switch(id, id, false, READING)));
        }
        assert!(specs.insert_fixed(&KEPT, 0));
        assert_eq!(specs.owned.capacity(), 3, "made exactly sized");
        specs.get_mut("t.kept").unwrap().scoped = true;
        assert_eq!(
            specs.owned.capacity(),
            4,
            "one more place, not twice as many"
        );
        assert!(
            specs.get("t.kept").unwrap().scoped,
            "the copy is the one changed"
        );
    }
}
