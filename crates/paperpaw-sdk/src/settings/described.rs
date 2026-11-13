//! Settings their plugin describes on demand ([`Described`]): of a plugin
//! loaded from the card only each setting's default and a byte of flags
//! stay on the heap. A spec (its id, texts and choices) is made from the
//! plugin's own data when it is asked for, and let go again.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::{Cell, OnceCell};
use core::fmt;
use core::ops::ControlFlow;

use super::{Level, SettingSpec, SettingValue};
use crate::Id;

/// Why a described setting's spec could not be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DescribeError {
    /// The heap has no room for it now: it is there, and may be made later.
    NoMemory,
    /// What described it cannot be read (any more): the setting is as if
    /// not there.
    Gone,
    /// What is described cannot be kept this way, and why.
    Invalid(&'static str),
}

/// Describes a plugin's settings where the plugin keeps them (a host reads
/// the records of a plugin loaded from the card), so that their specs need
/// not stay on the heap. The settings and their order never change.
///
/// Neither function may ask [`Settings`](super::Settings) for anything.
pub trait Described {
    /// How many settings there are.
    fn count(&self) -> usize;

    /// Gives `visit` the name of setting `at`, its id after `<plugin>.`,
    /// without making anything.
    fn name(&self, at: usize, visit: &mut dyn FnMut(&str)) -> Result<(), DescribeError>;

    /// Makes the spec of setting `at`, owning all its texts, without an
    /// [extra](SettingSpec::extra). Never stops for lack of memory:
    /// [`DescribeError::NoMemory`] where the heap has no room.
    fn make(&self, at: usize) -> Result<SettingSpec, DescribeError>;
}

const SCOPED: u8 = 1;
const GUARDED: u8 = 1 << 1;
const KEPT_ON_RESET: u8 = 1 << 2;
const ADVANCED: u8 = 1 << 3;

/// The most settings all plugins may describe: a place of 14 bits each.
pub(super) const PLACES: usize = 1 << 14;

/// One made spec in a box of its own, made without stopping where the heap
/// has no room.
type Made = Box<[SettingSpec]>;

/// The described settings of one plugin: what stays on the heap of each
/// (its default, its flags and its name's hash) and, until the next rest,
/// the specs that were asked for.
pub(super) struct Block {
    plugin: Id,
    source: Box<dyn Described>,
    /// The place of its first setting: places count on through the blocks.
    first: u16,
    /// Each setting's default as it is now: what a made spec gets.
    defaults: Box<[SettingValue]>,
    flags: Box<[u8]>,
    hashes: Box<[u16]>,
    made: OnceCell<Box<[OnceCell<Made>]>>,
}

impl fmt::Debug for Block {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Block")
            .field("plugin", &self.plugin)
            .field("first", &self.first)
            .field("defaults", &self.defaults)
            .finish_non_exhaustive()
    }
}

/// A name's hash: what is compared before the name is read.
fn hash(name: &str) -> u16 {
    let hash = paperpaw_digest::fnv1a64(name.as_bytes());
    (hash ^ (hash >> 16) ^ (hash >> 32) ^ (hash >> 48)) as u16
}

/// `value` again, `None` where the heap has no room for its text.
fn try_value(value: &SettingValue) -> Option<SettingValue> {
    Some(match value {
        SettingValue::Bool(value) => SettingValue::Bool(*value),
        SettingValue::Int(value) => SettingValue::Int(*value),
        SettingValue::Text(alloc::borrow::Cow::Borrowed(text)) => {
            SettingValue::Text(alloc::borrow::Cow::Borrowed(text))
        }
        SettingValue::Text(alloc::borrow::Cow::Owned(text)) => {
            let mut copy = String::new();
            copy.try_reserve_exact(text.len()).ok()?;
            copy.push_str(text);
            SettingValue::Text(alloc::borrow::Cow::Owned(copy))
        }
    })
}

/// An empty list with room for exactly `len`.
fn exactly<T>(len: usize) -> Result<Vec<T>, DescribeError> {
    let mut list = Vec::new();
    list.try_reserve_exact(len)
        .map_err(|_| DescribeError::NoMemory)?;
    Ok(list)
}

/// `list` as it is, boxed: full, so that nothing is allocated for it.
fn boxed<T>(list: Vec<T>) -> Result<Box<[T]>, DescribeError> {
    if list.len() != list.capacity() {
        return Err(DescribeError::NoMemory);
    }
    Ok(list.into_boxed_slice())
}

impl Block {
    pub(super) fn plugin(&self) -> &Id {
        &self.plugin
    }

    pub(super) fn len(&self) -> usize {
        self.flags.len()
    }

    pub(super) fn first(&self) -> u16 {
        self.first
    }

    /// The default of setting `at` as it is now.
    pub(super) fn default_now(&self, at: usize) -> &SettingValue {
        &self.defaults[at]
    }

    pub(super) fn scoped_flag(&self, at: usize) -> bool {
        self.flags[at] & SCOPED != 0
    }

    pub(super) fn guarded_flag(&self, at: usize) -> bool {
        self.flags[at] & GUARDED != 0
    }

    /// Which of its settings is called `name`. The hash is compared first;
    /// the name is then read where the plugin keeps it.
    fn find(&self, name: &str) -> Option<usize> {
        let hash = hash(name);
        self.hashes.iter().enumerate().find_map(|(at, other)| {
            let mut same = false;
            (*other == hash
                && self
                    .source
                    .name(at, &mut |other| same = other == name)
                    .is_ok()
                && same)
                .then_some(at)
        })
    }

    /// The spec of setting `at` if it was made and is still kept.
    pub(super) fn kept(&self, at: usize) -> Option<&SettingSpec> {
        self.made.get()?.get(at)?.get()?.first()
    }

    /// Makes the spec of setting `at`, with the default and the flags kept
    /// here. Nothing is kept of it.
    pub(super) fn make(&self, at: usize) -> Result<SettingSpec, DescribeError> {
        let mut spec = self.source.make(at)?;
        // What was described then must still be: a plugin can change its data.
        let named = spec
            .id
            .as_str()
            .strip_prefix(self.plugin.as_str())
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(|name| self.hashes.get(at) == Some(&hash(name)));
        if !named || spec.extra.is_some() {
            return Err(DescribeError::Gone);
        }
        let flags = self.flags[at];
        // Of another kind now than the default kept is of: not the setting
        // that was described.
        if !spec.kind.accepts(&self.defaults[at]) {
            return Err(DescribeError::Gone);
        }
        spec.default = try_value(&self.defaults[at]).ok_or(DescribeError::NoMemory)?;
        spec.scoped = flags & SCOPED != 0;
        spec.guarded = flags & GUARDED != 0;
        spec.kept_on_reset = flags & KEPT_ON_RESET != 0;
        spec.level = if flags & ADVANCED != 0 {
            Level::Advanced
        } else {
            Level::Basic
        };
        Ok(spec)
    }

    /// The spec of setting `at`, made if need be and kept until the next
    /// rest; `made` is told that one is kept.
    fn spec(&self, at: usize, made: &Cell<bool>) -> Result<&SettingSpec, DescribeError> {
        if let Some(spec) = self.kept(at) {
            return Ok(spec);
        }
        if self.made.get().is_none() {
            let mut cells = exactly(self.len())?;
            cells.resize_with(self.len(), OnceCell::new);
            // Set by no one else since `get`: nothing runs in between.
            self.made.set(boxed(cells)?).ok();
            made.set(true);
        }
        let cell = self
            .made
            .get()
            .and_then(|cells| cells.get(at))
            .ok_or(DescribeError::Gone)?;
        let mut one = exactly(1)?;
        one.push(self.make(at)?);
        // `make` asks the settings for nothing, so the cell is still empty.
        cell.set(boxed(one)?).ok();
        cell.get()
            .and_then(|one| one.first())
            .ok_or(DescribeError::Gone)
    }

    /// The default of setting `at` to change; the spec made with the old
    /// one is let go.
    fn default_mut(&mut self, at: usize) -> &mut SettingValue {
        if let Some(cell) = self.made.get_mut().and_then(|cells| cells.get_mut(at)) {
            cell.take();
        }
        &mut self.defaults[at]
    }
}

/// A plugin's described settings, checked and ready to be defined
/// ([`Settings::define_described`](super::Settings::define_described)).
#[derive(Debug)]
pub struct DescribedSettings(Block);

impl DescribedSettings {
    /// Reads what `source` describes of `plugin`'s settings, each spec made
    /// once and let go: its default and flags are kept.
    ///
    /// # Errors
    /// The heap has no room ([`DescribeError::NoMemory`]), `source` cannot be
    /// read, or a setting cannot be kept this way
    /// ([`DescribeError::Invalid`]): an id that is not `<plugin>.<name>`, a
    /// name twice, a default that is not of its kind, or an extra (the
    /// caller defines such settings as they are, with
    /// [`Settings::define`](super::Settings::define)).
    pub fn new(plugin: Id, source: Box<dyn Described>) -> Result<Self, DescribeError> {
        let count = source.count();
        if count > PLACES {
            return Err(DescribeError::Invalid("too many settings"));
        }
        // An id is told apart at its first dot: the plugin's has none.
        if plugin.as_str().contains('.') {
            return Err(DescribeError::Invalid("a plugin id with a dot"));
        }
        let mut defaults = exactly(count)?;
        let mut flags = exactly(count)?;
        let mut hashes: Vec<u16> = exactly(count)?;
        for at in 0..count {
            let spec = source.make(at)?;
            let name = spec
                .id
                .as_str()
                .strip_prefix(plugin.as_str())
                .and_then(|rest| rest.strip_prefix('.'))
                .ok_or(DescribeError::Invalid("a setting of another plugin"))?;
            let mut same = false;
            source.name(at, &mut |other| same = other == name)?;
            if !same {
                return Err(DescribeError::Invalid("a setting under another name"));
            }
            if spec.extra.is_some() {
                return Err(DescribeError::Invalid("a setting with an extra"));
            }
            if !spec.kind.accepts(&spec.default) {
                return Err(DescribeError::Invalid(
                    "default value does not match the setting kind",
                ));
            }
            let hash = hash(name);
            for (earlier, other) in hashes.iter().enumerate() {
                let mut twice = false;
                if *other == hash {
                    source.name(earlier, &mut |other| twice = other == name)?;
                }
                if twice {
                    return Err(DescribeError::Invalid("a setting declared twice"));
                }
            }
            hashes.push(hash);
            flags.push(
                (if spec.scoped { SCOPED } else { 0 })
                    | (if spec.guarded { GUARDED } else { 0 })
                    | (if spec.kept_on_reset { KEPT_ON_RESET } else { 0 })
                    | (if spec.level == Level::Advanced {
                        ADVANCED
                    } else {
                        0
                    }),
            );
            defaults.push(spec.default);
        }
        Ok(Self(Block {
            plugin,
            source,
            first: 0,
            defaults: boxed(defaults)?,
            flags: boxed(flags)?,
            hashes: boxed(hashes)?,
            made: OnceCell::new(),
        }))
    }

    /// How many settings these are.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.len() == 0
    }

    /// The plugin whose settings these are.
    pub fn plugin(&self) -> &Id {
        &self.0.plugin
    }

    /// Gives `visit` each setting's name (its id is `<plugin>.<name>`) in
    /// their order, until it answers `false`. Nothing is made.
    pub(super) fn names(&self, visit: &mut dyn FnMut(&str) -> bool) -> Result<(), DescribeError> {
        for at in 0..self.0.len() {
            let mut on = true;
            self.0.source.name(at, &mut |name| on = visit(name))?;
            if !on {
                break;
            }
        }
        Ok(())
    }
}

/// The id `<plugin>.<name>`; `None` where it is none or the heap has no room.
#[cfg(any(test, feature = "kernel"))]
fn joined(plugin: &str, name: &str) -> Option<Id> {
    let mut id = String::new();
    id.try_reserve_exact(plugin.len() + 1 + name.len()).ok()?;
    id.push_str(plugin);
    id.push('.');
    id.push_str(name);
    Id::try_parse(&id).ok()
}

/// Where a described setting is: its block and its place there.
pub(super) type Found<'a> = (&'a Block, usize);

/// Every plugin's described settings.
#[derive(Debug, Default)]
pub(super) struct Blocks {
    blocks: Vec<Block>,
    /// Whether a spec was made and is kept: what [`Blocks::rest`] lets go.
    made: Cell<bool>,
    /// Whether a spec asked for was left out for lack of room since the
    /// last [`Blocks::rest`].
    short: Cell<bool>,
}

impl Blocks {
    /// The described setting `id`.
    pub(super) fn find(&self, id: &str) -> Option<Found<'_>> {
        if self.blocks.is_empty() {
            return None;
        }
        let (plugin, name) = id.split_once('.')?;
        self.find_named(plugin, name)
    }

    /// The described setting `<plugin>.<name>`.
    pub(super) fn find_named(&self, plugin: &str, name: &str) -> Option<Found<'_>> {
        self.blocks
            .iter()
            .filter(|block| block.plugin.as_str() == plugin)
            .find_map(|block| Some((block, block.find(name)?)))
    }

    /// The setting at `place`, one that [`Blocks::add`] gave.
    pub(super) fn at(&self, place: u16) -> Found<'_> {
        let block = self.blocks.partition_point(|block| block.first <= place) - 1;
        let block = &self.blocks[block];
        (block, usize::from(place - block.first))
    }

    /// The spec of `found`, made if need be and kept until the next rest.
    pub(super) fn spec<'a>(
        &'a self,
        (block, at): Found<'a>,
    ) -> Result<&'a SettingSpec, DescribeError> {
        block.spec(at, &self.made)
    }

    /// The default of setting `id` to change, if it is described.
    pub(super) fn default_mut(&mut self, id: &str) -> Option<&mut SettingValue> {
        let (plugin, name) = id.split_once('.')?;
        self.blocks
            .iter_mut()
            .filter(|block| block.plugin.as_str() == plugin)
            .find_map(|block| {
                let at = block.find(name)?;
                Some(block.default_mut(at))
            })
    }

    /// The place the next block's first setting gets, `None` where
    /// `additional` more settings find no places.
    pub(super) fn next(&self, additional: usize) -> Option<u16> {
        let next = self
            .blocks
            .last()
            .map_or(0, |last| usize::from(last.first) + last.len());
        (next + additional <= PLACES).then_some(next as u16)
    }

    pub(super) fn try_reserve(&mut self) -> Result<(), alloc::collections::TryReserveError> {
        self.blocks.try_reserve_exact(1)
    }

    /// Adds `described` at the place [`Blocks::next`] gave, for which
    /// there is room.
    pub(super) fn add(&mut self, described: DescribedSettings, first: u16) {
        let mut block = described.0;
        block.first = first;
        self.blocks.push(block);
    }

    /// How far the place `place` moves down when `plugin`'s blocks go;
    /// `None` for a place of one of them.
    pub(super) fn moved(&self, place: u16, plugin: &str) -> Option<u16> {
        let mut gap = 0;
        for block in &self.blocks {
            if block.first > place {
                break;
            }
            let goes = block.plugin.as_str() == plugin;
            if goes && usize::from(place - block.first) < block.len() {
                return None;
            }
            if goes {
                gap += block.len() as u16;
            }
        }
        Some(gap)
    }

    /// Forgets `plugin`'s blocks; the others move down over the gaps.
    pub(super) fn remove_plugin(&mut self, plugin: &str) {
        let mut gap = 0;
        for block in &mut self.blocks {
            if block.plugin.as_str() == plugin {
                gap += block.len() as u16;
            } else {
                block.first -= gap;
            }
        }
        self.blocks.retain(|block| block.plugin.as_str() != plugin);
    }

    /// Notes a spec left out for lack of room where nobody is told of it.
    pub(super) fn note_short(&self) {
        self.short.set(true);
    }

    pub(super) fn went_short(&self) -> bool {
        self.short.get()
    }

    /// Lets go of every made spec. Nothing to do where none was made.
    pub(super) fn rest(&mut self) {
        self.short.set(false);
        if !self.made.replace(false) {
            return;
        }
        for block in &mut self.blocks {
            block.made.take();
        }
    }

    /// Whether a made spec is kept now.
    pub(super) fn any_made(&self) -> bool {
        self.made.get()
    }

    pub(super) fn shrink_to_fit(&mut self) {
        self.blocks.shrink_to_fit();
    }
}

#[cfg(any(test, feature = "kernel"))]
impl super::Settings {
    /// Whether `described` can be defined: `Duplicate` where a setting of
    /// one of its ids is there already, `Invalid` where no places are left
    /// or its names cannot be read. Nothing is made.
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn check_described(
        &self,
        described: &DescribedSettings,
    ) -> Result<(), crate::plugin::RegisterError> {
        use crate::plugin::RegisterError;
        let invalid = |message: &'static str| RegisterError::Invalid {
            message: alloc::borrow::Cow::Borrowed(message),
        };
        if !self.specs.has_room_for_described(described) {
            return Err(invalid("no place left for another setting"));
        }
        // No text is made for an id unless it is there twice, so that
        // defining what was checked asks the heap for nothing.
        let plugin = described.plugin().as_str();
        let mut twice = None;
        described
            .names(&mut |name| {
                if self.specs.has_named(plugin, name) {
                    twice = Some(joined(plugin, name));
                }
                twice.is_none()
            })
            .map_err(|_| invalid("a plugin's settings cannot be read now"))?;
        match twice {
            None => Ok(()),
            Some(Some(id)) => Err(RegisterError::Duplicate { id }),
            Some(None) => Err(invalid("a setting declared twice")),
        }
    }

    /// Makes room for one more plugin's `settings` described settings, so
    /// that [`Settings::define_described`] cannot fail for lack of memory.
    /// `others` is how many settings room was made for before
    /// ([`Settings::try_reserve_with_fixed`]) that are not defined yet: the
    /// room made here is next to theirs.
    ///
    /// # Errors
    /// The heap refused the room (some may be kept).
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn try_reserve_described(
        &mut self,
        settings: usize,
        others: usize,
    ) -> Result<(), alloc::collections::TryReserveError> {
        self.specs.try_reserve_described(settings, others)
    }

    /// Registers the settings a plugin describes on demand: of each only
    /// its default and flags are kept, and its spec is made when asked for
    /// ([`Settings::try_spec`], [`Settings::walk`]). All of them or none.
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn define_described(
        &mut self,
        described: DescribedSettings,
    ) -> Result<(), crate::plugin::RegisterError> {
        self.check_described(&described)?;
        if !self.specs.insert_described(described) {
            return Err(crate::plugin::RegisterError::Invalid {
                message: alloc::borrow::Cow::Borrowed("no place left for another setting"),
            });
        }
        Ok(())
    }

    /// [`Settings::spec`] that tells an unknown setting (`None`) from one
    /// whose spec the heap has no room for now.
    ///
    /// # Errors
    /// [`SettingError::Full`]: the setting is there; the same may work later.
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn try_spec(&self, id: &str) -> Result<Option<&SettingSpec>, super::SettingError> {
        self.spec_or_full(id)
    }

    /// Whether a setting of exactly this id is defined (a placement's id is
    /// none). Nothing is made for it.
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn defines(&self, id: &str) -> bool {
        self.specs.has(id)
    }

    /// Whether setting `id` (for a placement's id the setting's own, as
    /// [`Settings::spec`]) is defined and may differ per scope
    /// ([`SettingSpec::scoped`]). Nothing is made for it.
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn defines_scoped(&self, id: &str) -> bool {
        self.located(id).is_some_and(super::specs::At::scoped_flag)
    }

    /// Whether setting `id` (as [`Settings::defines_scoped`] finds it) is
    /// defined and guarded ([`SettingSpec::guarded`]). Nothing is made for
    /// it, so no lack of room makes a guarded setting look like any other.
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn defines_guarded(&self, id: &str) -> bool {
        self.located(id).is_some_and(super::specs::At::guarded_flag)
    }

    /// Whether setting `id` (as [`Settings::defines_scoped`] finds it) is
    /// defined and a secret. Nothing is made for it.
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn defines_secret(&self, id: &str) -> bool {
        self.located(id).is_some_and(super::specs::At::secret_flag)
    }

    /// Whether [`Settings::spec`] finds a setting for `id`, a placement's
    /// too. Nothing is made for it.
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn knows(&self, id: &str) -> bool {
        self.located(id).is_some()
    }

    fn located(&self, id: &str) -> Option<super::specs::At<'_>> {
        let plain = crate::id::unplaced(id);
        self.specs.where_is(plain.as_deref().unwrap_or(id))
    }

    /// The specs kept as they are, in registration order: all but those
    /// their plugins describe, so nothing is made. Every setting with an
    /// [extra](SettingSpec::extra) is one of them.
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn kept_specs(&self) -> impl Iterator<Item = &SettingSpec> {
        self.specs.kept()
    }

    /// Lets go of the specs made for [`Settings::spec`] and
    /// [`Settings::specs`] of settings their plugins describe: the kernel
    /// rests at the end of each round in which one was made.
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn rest(&mut self) {
        self.specs.rest();
    }

    /// Whether a spec made for a described setting is kept now (what
    /// [`Settings::rest`] lets go).
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn any_made(&self) -> bool {
        self.specs.any_made()
    }

    /// Whether [`Settings::spec`] or [`Settings::specs`] left out a setting
    /// for lack of room since the last [`Settings::rest`]: the kernel says
    /// so at the end of the round, as their callers cannot tell.
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn went_short(&self) -> bool {
        self.specs.went_short()
    }
}

impl super::Settings {
    /// Gives `visit` the spec of each setting `pick` takes, in registration
    /// order, until it breaks. Unlike [`Settings::specs`] it keeps nothing:
    /// the spec of a setting of a plugin loaded from the card is made for
    /// the visit and let go again, one at a time.
    ///
    /// # Errors
    /// [`SettingError::Full`](super::SettingError::Full): the heap has no
    /// room for a spec now; the walk stopped there, so what it gave is not
    /// all there is.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn walk(
        &self,
        pick: super::Pick<'_>,
        visit: &mut dyn FnMut(&SettingSpec) -> ControlFlow<()>,
    ) -> Result<(), super::SettingError> {
        self.specs.walk(pick, visit)
    }

    /// [`Settings::walk`] over all `pick` takes.
    ///
    /// # Errors
    /// As [`Settings::walk`].
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn each(
        &self,
        pick: super::Pick<'_>,
        visit: impl FnMut(&SettingSpec),
    ) -> Result<(), super::SettingError> {
        self.each_spec(pick, visit)
    }

    /// The default of setting `id` as it is now, by its very id (none of
    /// a placement's): nothing is made for it.
    pub(crate) fn default_of(&self, id: &str) -> Option<&SettingValue> {
        self.specs.where_is(id).map(super::specs::At::default_now)
    }

    /// The kernel's `walk`, for what the SDK itself reads of the settings.
    pub(crate) fn walk_specs(
        &self,
        pick: super::Pick<'_>,
        visit: &mut dyn FnMut(&SettingSpec) -> ControlFlow<()>,
    ) -> Result<(), super::SettingError> {
        self.specs.walk(pick, visit)
    }

    /// [`Settings::walk_specs`] over all `pick` takes.
    pub(crate) fn each_spec(
        &self,
        pick: super::Pick<'_>,
        mut visit: impl FnMut(&SettingSpec),
    ) -> Result<(), super::SettingError> {
        self.specs.walk(pick, &mut |spec| {
            visit(spec);
            ControlFlow::Continue(())
        })
    }
}
