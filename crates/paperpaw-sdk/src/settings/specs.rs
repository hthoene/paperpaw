//! Where the settings' specs are kept ([`Specs`]).

use alloc::vec::Vec;
use core::ops::ControlFlow;

use super::described::{Blocks, DescribeError, DescribedSettings, Found};
use super::{SettingError, SettingSpec, SettingValue};

/// The settings' specs in registration order, found by id through an
/// index sorted by it: each id kept once. Specs kept in the program
/// (a `static` list of `SettingSpec`s, e.g. in flash) are held as the lists a plugin
/// gave ([`Registrar::add_settings`](crate::Registrar::add_settings)),
/// a reference to each list rather than to each spec; the others as they
/// are. Of the specs a plugin describes on demand
/// ([`Described`](super::Described)) only the places are kept here: they
/// are not in the index by id, and are found by their plugin and name.
#[derive(Debug, Default)]
pub(super) struct Specs {
    owned: Vec<SettingSpec>,
    /// The lists of fixed specs, each with the place of its first spec:
    /// the places of fixed specs count on through the lists.
    lists: Vec<(&'static [SettingSpec], u16)>,
    /// Every spec in registration order: a place in `owned`, among the
    /// fixed specs with [`FIXED`] set, or among the described ones with
    /// [`DESCRIBED`] set.
    order: Vec<u16>,
    /// The places of the made and the fixed specs, sorted by the specs' ids.
    by_id: Vec<u16>,
    described: Blocks,
}

/// Marks a place among the fixed specs.
const FIXED: u16 = 1 << 15;
/// Marks a place among the described specs (one that is not [`FIXED`]).
const DESCRIBED: u16 = 1 << 14;
/// Fixed specs a place of 15 bits holds: plenty, and two bytes a place on
/// small devices.
const PLACES: usize = FIXED as usize;
/// Specs made at run time a place of 14 bits holds (as many may be
/// described).
const MADE_PLACES: usize = DESCRIBED as usize;

/// Where a setting is kept.
#[derive(Clone, Copy)]
pub(super) enum At<'a> {
    Spec(&'a SettingSpec),
    Described(Found<'a>),
}

impl<'a> At<'a> {
    /// Its default as it is now: nothing is made for it.
    pub(super) fn default(self) -> &'a SettingValue {
        match self {
            At::Spec(spec) => &spec.default,
            At::Described((block, at)) => block.default(at),
        }
    }

    pub(super) fn scoped(self) -> bool {
        match self {
            At::Spec(spec) => spec.scoped,
            At::Described((block, at)) => block.scoped(at),
        }
    }

    /// The setting it follows until it is set (a described one follows
    /// none: it has no extra).
    pub(super) fn follows(self) -> Option<&'a crate::Id> {
        match self {
            At::Spec(spec) => spec.extra().follows.as_ref(),
            At::Described(_) => None,
        }
    }
}

/// Which settings a walk visits ([`Settings::walk`](super::Settings::walk)).
/// The described ones of a plugin it leaves out are not made for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Pick<'a> {
    All,
    /// Those that may differ per scope ([`SettingSpec::scoped`]).
    Scoped,
    /// Those of one plugin (ids `<plugin>.…`).
    Of(&'a str),
    /// Those of any of these plugins.
    OfAny(&'a [&'a str]),
    /// Those with an [extra](SettingSpec::extra).
    WithExtra,
}

impl Pick<'_> {
    fn takes(self, spec: &SettingSpec) -> bool {
        match self {
            Pick::All => true,
            Pick::Scoped => spec.scoped,
            Pick::Of(plugin) => spec.id.namespace() == plugin,
            Pick::OfAny(plugins) => plugins.contains(&spec.id.namespace()),
            Pick::WithExtra => spec.extra.is_some(),
        }
    }

    fn takes_described(self, (block, at): Found<'_>) -> bool {
        match self {
            Pick::All => true,
            Pick::Scoped => block.scoped(at),
            Pick::Of(plugin) => block.plugin().as_str() == plugin,
            Pick::OfAny(plugins) => plugins.contains(&block.plugin().as_str()),
            Pick::WithExtra => false,
        }
    }
}

/// `Full` where the heap had no room; a setting that cannot be read any
/// more is as if not there.
fn full<T>(made: Result<T, DescribeError>) -> Result<Option<T>, SettingError> {
    match made {
        Ok(made) => Ok(Some(made)),
        Err(DescribeError::NoMemory) => Err(SettingError::Full),
        Err(_) => Ok(None),
    }
}

impl Specs {
    /// The spec at a place of `by_id`: a made or a fixed one.
    fn at(&self, place: u16) -> &SettingSpec {
        if place & FIXED == 0 {
            return &self.owned[usize::from(place)];
        }
        let place = place & !FIXED;
        let list = self.lists.partition_point(|(_, first)| *first <= place) - 1;
        let (specs, first) = self.lists[list];
        &specs[usize::from(place - first)]
    }

    /// What is at a place of `order`.
    fn place(&self, place: u16) -> At<'_> {
        if place & (FIXED | DESCRIBED) == DESCRIBED {
            At::Described(self.described.at(place & !DESCRIBED))
        } else {
            At::Spec(self.at(place))
        }
    }

    /// Where setting `id` is kept. Nothing is made.
    pub(super) fn locate(&self, id: &str) -> Option<At<'_>> {
        match self.find(id) {
            Ok(at) => Some(At::Spec(self.at(self.by_id[at]))),
            Err(_) => self.described.find(id).map(At::Described),
        }
    }

    /// Whether setting `id` is one its plugin describes.
    #[cfg(any(test, feature = "kernel"))]
    pub(super) fn is_described(&self, id: &str) -> bool {
        matches!(self.locate(id), Some(At::Described(_)))
    }

    /// Whether there is a setting `id`. Nothing is made.
    pub(super) fn has(&self, id: &str) -> bool {
        self.locate(id).is_some()
    }

    /// Where `id` is in `by_id`, or would go.
    fn find(&self, id: &str) -> Result<usize, usize> {
        self.by_id
            .binary_search_by(|&at| self.at(at).id.as_str().cmp(id))
    }

    /// The spec of setting `id`; a described one is made if need be and
    /// kept until the next rest ([`Specs::rest`]).
    pub(super) fn get(&self, id: &str) -> Result<Option<&SettingSpec>, SettingError> {
        match self.locate(id) {
            None => Ok(None),
            Some(At::Spec(spec)) => Ok(Some(spec)),
            Some(At::Described(found)) => full(self.described.spec(found)),
        }
    }

    /// Gives `f` the spec of setting `id`; a described one is made for it
    /// and let go again (unless it is kept already).
    pub(super) fn with<R>(
        &self,
        id: &str,
        f: impl FnOnce(&SettingSpec) -> R,
    ) -> Result<Option<R>, SettingError> {
        match self.locate(id) {
            None => Ok(None),
            Some(At::Spec(spec)) => Ok(Some(f(spec))),
            Some(At::Described((block, at))) => match block.kept(at) {
                Some(spec) => Ok(Some(f(spec))),
                None => Ok(full(block.make(at))?.map(|spec| f(&spec))),
            },
        }
    }

    /// Gives `visit` the specs `pick` takes, in registration order, until
    /// it breaks. A described one is made for it and let go again.
    pub(super) fn walk(
        &self,
        pick: Pick<'_>,
        visit: &mut dyn FnMut(&SettingSpec) -> ControlFlow<()>,
    ) -> Result<(), SettingError> {
        for &place in &self.order {
            let went = match self.place(place) {
                At::Spec(spec) if pick.takes(spec) => visit(spec),
                At::Described(found) if pick.takes_described(found) => {
                    let (block, at) = found;
                    match block.kept(at) {
                        Some(spec) => visit(spec),
                        None => match full(block.make(at))? {
                            Some(spec) => visit(&spec),
                            None => ControlFlow::Continue(()),
                        },
                    }
                }
                _ => ControlFlow::Continue(()),
            };
            if went.is_break() {
                break;
            }
        }
        Ok(())
    }

    /// The default of setting `id` to change, and nothing else of it: a
    /// fixed spec is copied first as in [`Specs::get_mut`]; a described one
    /// stays as it is.
    pub(super) fn default_mut(&mut self, id: &str) -> Result<&mut SettingValue, SettingError> {
        if self.find(id).is_ok() {
            return self.get_mut(id).map(|spec| &mut spec.default);
        }
        self.described.default_mut(id).ok_or(SettingError::Unknown)
    }

    /// Makes the described setting `id` one kept as it is, to be changed:
    /// its spec is made a last time. Nothing changes where no room is left.
    fn keep_described(&mut self, id: &str) -> Result<(), SettingError> {
        let Err(at) = self.find(id) else {
            return Ok(());
        };
        let (block, setting) = self.described.find(id).ok_or(SettingError::Unknown)?;
        let place = DESCRIBED | (block.first() + setting as u16);
        if self.owned.len() >= MADE_PLACES
            || self.owned.try_reserve_exact(1).is_err()
            || self.by_id.try_reserve_exact(1).is_err()
        {
            return Err(SettingError::Full);
        }
        let spec = full(block.make(setting))?.ok_or(SettingError::Unknown)?;
        let owned = self.owned.len() as u16;
        self.owned.push(spec);
        self.by_id.insert(at, owned);
        if let Some(o) = self.order.iter_mut().find(|o| **o == place) {
            *o = owned;
        }
        Ok(())
    }

    /// The spec `id` to change: a fixed one is copied first, which needs
    /// a place of its own, and a described one is kept as it is from then on.
    pub(super) fn get_mut(&mut self, id: &str) -> Result<&mut SettingSpec, SettingError> {
        self.keep_described(id)?;
        let at = self.find(id).map_err(|_| SettingError::Unknown)?;
        let place = self.by_id[at];
        let place = if place & FIXED == 0 {
            place
        } else if self.owned.len() >= MADE_PLACES {
            return Err(SettingError::Full);
        } else {
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
        if self.owned.len() >= MADE_PLACES {
            return false;
        }
        let place = self.owned.len() as u16;
        self.owned.push(spec);
        self.by_id.insert(at, place);
        self.order.push(place);
        true
    }

    /// Whether `described` finds places.
    pub(super) fn has_room_for_described(&self, described: &DescribedSettings) -> bool {
        self.described.next(described.len()).is_some()
    }

    /// Makes room for one more plugin's `settings` described settings;
    /// nothing changes when the heap refuses.
    pub(super) fn try_reserve_described(
        &mut self,
        settings: usize,
    ) -> Result<(), alloc::collections::TryReserveError> {
        self.described.try_reserve()?;
        self.order.try_reserve_exact(settings)
    }

    /// Adds `described`, none of whose ids is there yet; `false`, and
    /// nothing added, where no places or no room are left.
    pub(super) fn insert_described(&mut self, described: DescribedSettings) -> bool {
        let len = described.len();
        let Some(first) = self.described.next(len) else {
            return false;
        };
        if self.try_reserve_described(len).is_err() {
            return false;
        }
        self.described.add(described, first);
        self.order
            .extend((0..len as u16).map(|at| DESCRIBED | (first + at)));
        true
    }

    /// Lets go of the described specs that were made.
    pub(super) fn rest(&mut self) {
        self.described.rest();
    }

    /// Whether a described spec that was made is kept now.
    pub(super) fn any_made(&self) -> bool {
        self.described.any_made()
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
            .is_some_and(|total| total <= MADE_PLACES)
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
            described,
        } = self;
        let belongs = |owned: &[SettingSpec], place: u16| {
            if place & (FIXED | DESCRIBED) == DESCRIBED {
                return described.moved(place & !DESCRIBED, plugin).is_none();
            }
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
        // A made spec's place drops by the gaps before it, and so does a
        // described one's.
        for place in order.iter_mut().chain(by_id.iter_mut()) {
            if *place & (FIXED | DESCRIBED) == DESCRIBED {
                *place -= described.moved(*place & !DESCRIBED, plugin).unwrap_or(0);
            } else if *place & FIXED == 0 {
                let gaps = owned[..usize::from(*place)]
                    .iter()
                    .filter(|spec| spec.id.namespace() == plugin)
                    .count();
                *place -= gaps as u16;
            }
        }
        owned.retain(|spec| spec.id.namespace() != plugin);
        described.remove_plugin(plugin);
    }

    /// Every spec in registration order. The described ones are made and
    /// kept until the next rest; one the heap has no room for is left out.
    pub(super) fn iter(&self) -> impl Iterator<Item = &SettingSpec> {
        self.order
            .iter()
            .filter_map(|&place| match self.place(place) {
                At::Spec(spec) => Some(spec),
                At::Described(found) => self.described.spec(found).ok(),
            })
    }

    pub(super) fn shrink_to_fit(&mut self) {
        self.owned.shrink_to_fit();
        self.lists.shrink_to_fit();
        self.order.shrink_to_fit();
        self.by_id.shrink_to_fit();
        self.described.shrink_to_fit();
    }
}
