//! Borrowed declaration storage pending the draft-4 SDK registry boundary.
//!
//! Only committed construction batches may enter this store. Registry adapters
//! will add cross-plugin reference checks before prepare. No decoded view lives
//! beyond its visitor, and the retained owner supplies every image lookup.
//! Consumers must not rely on uniqueness or valid references after admission:
//! native plugins can change image bytes without a sandbox.

use super::{
    declaration_checks::Cap,
    declarations,
    registrations::{Record, host_records_decode},
    staged::DECLARATIONS_MAX_BYTES,
};
use alloc::{boxed::Box, rc::Rc, vec::Vec};
use core::mem::size_of;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_STATE};
use paperpaw_sdk::Id;

/// The bridge holds the module guard and excludes image release during a visit.
/// Methods must not invoke plugin callbacks or reenter the store.
pub(crate) trait ImageOwner {
    /// Host identity, unchanged within a source generation.
    fn plugin_id(&self) -> &str;
    fn generation(&self) -> u32;
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>));
    fn invalid_declaration(&self, error: i32);
    fn owner_id(&self) -> Result<u64, i32> {
        Err(ERR_STATE)
    }
    fn native_lease(&self) -> Result<super::native_instances::NativeLease, i32> {
        Err(ERR_STATE)
    }
}

/// Test fixtures may use Rc; native admission uses checked shared ownership.
enum OwnerRef {
    Existing(Rc<dyn ImageOwner>),
    Native(super::shared::Shared<Box<dyn ImageOwner>>),
}
impl OwnerRef {
    fn try_clone(&self) -> Result<Self, i32> {
        match self {
            Self::Existing(owner) => Ok(Self::Existing(Rc::clone(owner))),
            Self::Native(owner) => owner.try_clone().map(Self::Native).ok_or(ERR_NO_MEMORY),
        }
    }
}
impl core::ops::Deref for OwnerRef {
    type Target = dyn ImageOwner;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Existing(owner) => &**owner,
            Self::Native(owner) => &***owner,
        }
    }
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

impl DeclKey {
    /// The place of the record: its owner's slot, its batch, its position in it.
    /// Sorting by this gives plugin order, then record order.
    pub(crate) fn order(self) -> (u16, u16, u16) {
        (self.owner, self.batch, self.record)
    }
}

const KIND_COUNT: usize = 34;
/// The leading kinds with a per-kind record limit (`limits` in `charge`).
const LIMITED_KINDS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Group,
    Spec,
    Extra,
    Hyphenation,
    Face,
    Extension,
    Screen,
    Action,
    Widget,
    WidgetPlace,
    StatusItem,
    StatusLine,
    StatusPlace,
    MenuEntry,
    Icon,
    MenuNeed,
    MenuSection,
    MenuHide,
    MenuDescribe,
    // REG declarations (`registry.md` section 6). `setup.step` (category 8)
    // joins as the next variant, with `KIND_COUNT` one higher, when the host
    // admits it.
    SlotSpec,
    SlotOffer,
    KeyBind,
    SettingDefault,
    SettingChoice,
    PresetSpec,
    PresetValue,
    /// `stage.spec`: a stage the plugin owns.
    Stage,
    /// `registration.stage`: the plugin's token on a stage (any owner's).
    Subscription,
    /// `setup.step` (category 8).
    SetupStep,
    /// `input.spec` (category 16): an input or gesture of the plugin's own.
    InputSpec,
    /// `help.hint` (category 17): a hint for any screen.
    HelpHint,
    /// `format.spec` (category 31): a book format the plugin reads.
    Format,
    /// `reader.spec` (category 32): a reading app the plugin offers.
    Reader,
    /// `lookup.spec` (category 33): a lookup provider the plugin offers.
    Lookup,
}
const _: () = assert!(Kind::Lookup as usize + 1 == KIND_COUNT);

impl Kind {
    fn category(self) -> u8 {
        match self {
            Self::Group => 0,
            Self::Spec => 1,
            Self::Extra => 2,
            Self::Hyphenation => 3,
            Self::Face | Self::Extension => 4,
            Self::Screen => 5,
            Self::Action => 6,
            Self::Widget => 18,
            Self::WidgetPlace => 19,
            Self::StatusItem => 20,
            Self::StatusLine => 21,
            Self::StatusPlace => 22,
            Self::MenuEntry => 23,
            Self::Icon => 24,
            Self::MenuNeed => 25,
            Self::MenuSection => 26,
            Self::MenuHide => 27,
            Self::MenuDescribe => 28,
            Self::SlotSpec => 9,
            Self::SlotOffer => 10,
            Self::KeyBind => 11,
            Self::SettingDefault => 12,
            Self::SettingChoice => 13,
            Self::PresetSpec => 14,
            Self::PresetValue => 15,
            Self::Stage => super::declaration_index::STAGE_KIND,
            Self::Subscription => super::declaration_index::STAGE_SUBSCRIPTION_KIND,
            Self::SetupStep => 8,
            Self::InputSpec => 16,
            Self::HelpHint => 17,
            Self::Format => 31,
            Self::Reader => 32,
            Self::Lookup => 33,
        }
    }

    fn of(record: &Record<'_>) -> Option<Self> {
        Some(match record {
            Record::SettingGroup(_) => Self::Group,
            Record::SettingSpec(_) => Self::Spec,
            Record::SettingExtra(_) => Self::Extra,
            Record::ResourceHyphenation(_) => Self::Hyphenation,
            Record::ResourceFontFace(_) => Self::Face,
            Record::ResourceFontExtension(_) => Self::Extension,
            Record::ScreenSpec(_) => Self::Screen,
            Record::ActionSpec(_) => Self::Action,
            Record::WidgetSpec(_) => Self::Widget,
            Record::WidgetPlace(_) => Self::WidgetPlace,
            Record::StatusItem(_) => Self::StatusItem,
            Record::StatusLine(_) => Self::StatusLine,
            Record::StatusPlace(_) => Self::StatusPlace,
            Record::MenuEntry(_) => Self::MenuEntry,
            Record::IconSpec(_) => Self::Icon,
            Record::MenuNeed(_) => Self::MenuNeed,
            Record::MenuSection(_) => Self::MenuSection,
            Record::MenuHide(_) => Self::MenuHide,
            Record::MenuDescribe(_) => Self::MenuDescribe,
            Record::SlotSpec(_) => Self::SlotSpec,
            Record::SlotOffer(_) => Self::SlotOffer,
            Record::KeyBind(_) => Self::KeyBind,
            Record::SettingDefault(_) => Self::SettingDefault,
            Record::SettingChoice(_) => Self::SettingChoice,
            Record::PresetSpec(_) => Self::PresetSpec,
            Record::PresetValue(_) => Self::PresetValue,
            Record::StageSpec(_) => Self::Stage,
            Record::RegistrationStage(_) => Self::Subscription,
            Record::SetupStep(_) => Self::SetupStep,
            Record::InputSpec(_) => Self::InputSpec,
            Record::HelpHint(_) => Self::HelpHint,
            Record::FormatSpec(_) => Self::Format,
            Record::ReaderSpec(_) => Self::Reader,
            Record::LookupSpec(_) => Self::Lookup,
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
    /// The declaration cap `prepare` refused the plugin at, if it was one.
    pub(crate) cap: Option<super::declaration_checks::Cap>,
}
impl Allocations {
    pub(crate) fn reserve<T>(&mut self, target: &mut Vec<T>, count: usize) -> Result<(), i32> {
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
    source: OwnerRef,
    batches: Vec<DeclRef>,
    /// Every indexed record of the owner in one list: grouped by kind, and
    /// inside a kind sorted by name hash once the owner is prepared. The
    /// range of kind `k` is `starts[k]..starts[k + 1]`.
    slots: Vec<Slot>,
    starts: [u16; KIND_COUNT + 1],
    resident: usize,
    incarnation: u32,
    failure: Option<i32>,
    index_bytes: usize,
    image_entries: usize,
    /// Records of owned (not image-backed) batches, by kind, for the per-kind
    /// limits: only the first kinds have one (the rest share the aggregate
    /// byte cap), so the table does not grow as kinds are added.
    owned_counts: [u16; LIMITED_KINDS],
}
impl PendingDeclarations {
    pub(crate) fn plugin_id(&self) -> &str {
        self.id.as_str()
    }
    pub(crate) fn host_owner_id(&self) -> Result<u64, i32> {
        self.source.owner_id()
    }

    /// The slots of one kind.
    fn of_kind(&self, kind: Kind) -> &[Slot] {
        let k = kind as usize;
        &self.slots[usize::from(self.starts[k])..usize::from(self.starts[k + 1])]
    }

    /// Adds `slot` at the end of its kind's range. The caller has reserved
    /// room, so nothing here allocates.
    fn add_slot(&mut self, kind: Kind, slot: Slot) {
        let at = usize::from(self.starts[kind as usize + 1]);
        self.slots.insert(at, slot);
        // Each owner holds far fewer than 65,535 records (the declaration
        // byte cap); a saturated offset would only end a range early.
        for start in &mut self.starts[kind as usize + 1..] {
            *start = start.saturating_add(1);
        }
    }

    pub(crate) fn new(id: Id, source: Rc<dyn ImageOwner>) -> Self {
        Self {
            id,
            generation: source.generation(),
            source: OwnerRef::Existing(source),
            batches: Vec::new(),
            slots: Vec::new(),
            starts: [0; KIND_COUNT + 1],
            resident: 0,
            incarnation: 0,
            failure: None,
            index_bytes: 0,
            image_entries: 0,
            owned_counts: [0; LIMITED_KINDS],
        }
    }

    pub(crate) fn native(id: Id, source: impl ImageOwner + 'static) -> Result<Self, i32> {
        let source: Box<dyn ImageOwner> = super::shared::try_box(source).ok_or(ERR_NO_MEMORY)?;
        let source =
            OwnerRef::Native(super::shared::Shared::try_new(source).map_err(|_| ERR_NO_MEMORY)?);
        Ok(Self {
            id,
            generation: source.generation(),
            source,
            batches: Vec::new(),
            slots: Vec::new(),
            starts: [0; KIND_COUNT + 1],
            resident: 0,
            incarnation: 0,
            failure: None,
            index_bytes: 0,
            image_entries: 0,
            owned_counts: [0; LIMITED_KINDS],
        })
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
        let source = self.source.try_clone()?;
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
        let decoded = host_records_decode(bytes)?;
        declarations::validate_batch(self.id.as_str(), &decoded)?;
        let offsets = super::raw_declarations::offsets(bytes, decoded.len())?;
        let mut counts = [0usize; KIND_COUNT];
        let mut kinds = [None; super::registrations::REGISTRATION_MAX_RECORDS];
        for (index, record) in decoded.iter().enumerate() {
            if let Some(kind) = Kind::of(&record) {
                kinds[index] = Some((
                    kind,
                    super::declaration_index::record_hash(record).ok_or(ERR_INVALID)?,
                ));
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
        if self.of_kind(Kind::Subscription).len() + counts[Kind::Subscription as usize]
            > super::declaration_checks::STAGE_SUBSCRIPTIONS_MAX
        {
            return Err(paperpaw_native::host_glue::ERR_FULL);
        }
        let (resident, index_bytes, image_entries) = self.charge(counts, image, bytes.len())?;
        let reference = match reference {
            Some(reference) => reference,
            None => input.own(allocations)?,
        };
        allocations.reserve(&mut self.batches, 1)?;
        allocations.reserve(&mut self.slots, counts.iter().sum())?;
        for (record, kind) in kinds.into_iter().enumerate() {
            if let Some((kind, hash)) = kind {
                // The decoder bounds each batch to eight records.
                self.add_slot(
                    kind,
                    Slot {
                        hash,
                        batch,
                        record: offsets[record],
                    },
                );
            }
        }
        self.batches.push(reference);
        self.resident = resident;
        self.index_bytes = index_bytes;
        self.image_entries = image_entries;
        if !image {
            for (total, count) in self.owned_counts.iter_mut().zip(counts) {
                *total = total.saturating_add(u16::try_from(count).unwrap_or(u16::MAX));
            }
        }
        Ok(())
    }

    fn charge(
        &self,
        counts: [usize; KIND_COUNT],
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
            let limits: [usize; KIND_COUNT] = [
                GROUPS_MAX,
                SPECS_MAX,
                EXTRAS_MAX,
                HYPHENATIONS_MAX,
                FONT_FACES_MAX,
                FONT_EXTENSIONS_MAX,
                // Screen, action and UIX copies share the aggregate byte cap.
                usize::MAX,
                usize::MAX,
                usize::MAX,
                usize::MAX,
                usize::MAX,
                usize::MAX,
                usize::MAX,
                // Menus and icons share the aggregate byte cap too.
                usize::MAX,
                usize::MAX,
                usize::MAX,
                usize::MAX,
                usize::MAX,
                usize::MAX,
                // REG declarations share the aggregate byte cap as well.
                usize::MAX,
                usize::MAX,
                usize::MAX,
                usize::MAX,
                usize::MAX,
                usize::MAX,
                usize::MAX,
                // Stage specs and subscriptions: the plugin's eight
                // subscriptions are checked at commit (`ERR_FULL`).
                usize::MAX,
                usize::MAX,
                // `setup.step` shares the aggregate byte cap.
                usize::MAX,
                // So do `input.spec` and `help.hint`.
                usize::MAX,
                usize::MAX,
                // And `format.spec`, `reader.spec` and `lookup.spec` (their
                // counts are checked at commit).
                usize::MAX,
                usize::MAX,
                usize::MAX,
            ];
            if self
                .owned_counts
                .iter()
                .zip(counts)
                .zip(limits)
                .any(|((&old, new), limit)| usize::from(old) + new > limit)
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
    /// Owners taken out so far.
    removed: u32,
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
        for kind in 0..KIND_COUNT {
            let range = usize::from(pending.starts[kind])..usize::from(pending.starts[kind + 1]);
            super::declaration_index::sort_by_key(&mut pending.slots[range], |entry| entry.hash);
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
        // At most 32 formats on the device (`document.md` section 1): the
        // plugin's own 8 are checked with its batch; here the others count.
        let others: usize = self
            .owners
            .iter()
            .enumerate()
            .filter(|(index, owner)| {
                *index != at
                    && owner
                        .as_ref()
                        .is_some_and(|owner| owner.source.generation() == owner.generation)
            })
            .filter_map(|(_, owner)| owner.as_ref())
            .map(|owner| owner.of_kind(Kind::Format).len())
            .sum();
        if others + pending.of_kind(Kind::Format).len() > super::format_index::DEVICE {
            allocations.cap = Some(Cap::of(Cap::FORMATS, super::format_index::DEVICE));
            return Err(paperpaw_native::host_glue::ERR_FULL);
        }
        // And 16 reading apps on the device, counted the same way.
        let readers: usize = self
            .owners
            .iter()
            .enumerate()
            .filter(|(index, owner)| {
                *index != at
                    && owner
                        .as_ref()
                        .is_some_and(|owner| owner.source.generation() == owner.generation)
            })
            .filter_map(|(_, owner)| owner.as_ref())
            .map(|owner| owner.of_kind(Kind::Reader).len())
            .sum();
        if readers + pending.of_kind(Kind::Reader).len() > super::format_index::READERS_DEVICE {
            allocations.cap = Some(Cap::of(Cap::READERS, super::format_index::READERS_DEVICE));
            return Err(paperpaw_native::host_glue::ERR_FULL);
        }
        // And 32 lookup providers on the device (`document.md` section 7).
        let providers: usize = self
            .owners
            .iter()
            .enumerate()
            .filter(|(index, owner)| {
                *index != at
                    && owner
                        .as_ref()
                        .is_some_and(|owner| owner.source.generation() == owner.generation)
            })
            .filter_map(|(_, owner)| owner.as_ref())
            .map(|owner| owner.of_kind(Kind::Lookup).len())
            .sum();
        if providers + pending.of_kind(Kind::Lookup).len()
            > super::declaration_checks::LOOKUPS_DEVICE
        {
            allocations.cap = Some(Cap::of(
                Cap::LOOKUPS,
                super::declaration_checks::LOOKUPS_DEVICE,
            ));
            return Err(paperpaw_native::host_glue::ERR_FULL);
        }
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
                self.removed = self.removed.wrapping_add(1);
            }
        }
    }

    /// A number that differs after every change to what the store declares:
    /// an owner added or replaced, or taken out. What was read from the store
    /// is good while it stays the same (an owner that stops without being
    /// taken out is not seen here). The input filter list leaves a filter out
    /// whose calls failed three inputs in a row and keeps that until this
    /// number changes (`input_chain::FilterCache`).
    pub(crate) fn revision(&self) -> u64 {
        u64::from(self.incarnation) << 32 | u64::from(self.removed)
    }

    pub(crate) fn prune_stopped_native(&mut self) {
        for owner in &mut self.owners {
            if owner.as_ref().is_some_and(|owner| {
                owner.source.owner_id().is_ok() && owner.source.generation() != owner.generation
            }) {
                *owner = None;
                self.removed = self.removed.wrapping_add(1);
            }
        }
    }

    pub(crate) fn remove_host_owner(&mut self, key: u64) {
        for owner in &mut self.owners {
            if owner
                .as_ref()
                .is_some_and(|owner| owner.source.owner_id() == Ok(key))
            {
                *owner = None;
                self.removed = self.removed.wrapping_add(1);
            }
        }
    }

    pub(crate) fn native_lease(
        &self,
        key: u64,
    ) -> Result<super::native_instances::NativeLease, i32> {
        let owner = self
            .owners
            .iter()
            .flatten()
            .find(|owner| owner.source.owner_id() == Ok(key))
            .filter(|owner| owner.source.generation() == owner.generation)
            .ok_or(paperpaw_native::host_glue::ERR_NOT_FOUND)?;
        owner.source.native_lease()
    }

    /// The plugin id of the live owner `key`.
    pub(crate) fn plugin_id_of_owner(&self, key: u64) -> Option<&str> {
        self.owners
            .iter()
            .flatten()
            .find(|owner| owner.source.owner_id() == Ok(key))
            .filter(|owner| owner.source.generation() == owner.generation)
            .map(PendingDeclarations::plugin_id)
    }

    pub(crate) fn host_owner(&self, plugin: &str) -> Result<u64, i32> {
        let owner = self
            .owners
            .iter()
            .flatten()
            .find(|owner| owner.id.as_str() == plugin)
            .ok_or(paperpaw_native::host_glue::ERR_NOT_FOUND)?;
        if owner.source.generation() != owner.generation {
            return Err(paperpaw_native::host_glue::ERR_NOT_FOUND);
        }
        owner.source.owner_id()
    }

    pub(crate) fn all_keys(&self, allocations: &mut Allocations) -> Result<Vec<DeclKey>, i32> {
        let mut keys = Vec::new();
        for (index, owner) in self.owners.iter().enumerate() {
            let Some(owner) = owner else { continue };
            if owner.source.generation() != owner.generation {
                continue;
            }
            allocations.reserve(&mut keys, owner.slots.len())?;
            for slot in &owner.slots {
                keys.push(DeclKey {
                    owner: u16::try_from(index).map_err(|_| ERR_NO_SPACE)?,
                    batch: slot.batch,
                    record: slot.record,
                    generation: owner.generation,
                    incarnation: owner.incarnation,
                });
            }
        }
        Ok(keys)
    }

    /// The plugin that owns the record `key` names.
    pub(crate) fn plugin_of(&self, key: DeclKey) -> Option<&str> {
        self.owners
            .get(usize::from(key.owner))
            .and_then(Option::as_ref)
            .filter(|owner| owner.incarnation == key.incarnation)
            .map(|owner| owner.id.as_str())
    }

    pub(crate) fn keys(&self, kind: Kind, visit: &mut dyn FnMut(DeclKey)) {
        for (index, owner) in self.owners.iter().enumerate() {
            let Some(owner) = owner else {
                continue;
            };
            if owner.source.generation() != owner.generation {
                continue;
            }
            for slot in owner.of_kind(kind) {
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
        self.find_parts(plugin, kind, &[name], visit)
    }

    /// Lookup by every key field of the kind (one for a plain name; a slot
    /// offer is `[slot, screen]`, a key binding `[layer, input, when_key,
    /// when_value]`, a choice `[key, value]`, a preset value `[preset, key]`).
    pub(crate) fn find_parts(
        &self,
        plugin: &str,
        kind: Kind,
        parts: &[&str],
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
        let category = kind.category();
        let hash = super::declaration_index::composite_hash(category, parts);
        let slots = owner.of_kind(kind);
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
                if super::declaration_index::matches_parts(record, category, parts) {
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
        self.visit_checked(key, true, visit)
    }

    /// Visit a retained record without requiring a named-kind index entry.
    /// The visitor must not invoke plugins or reenter the declaration store.
    pub(crate) fn visit_record(
        &self,
        key: DeclKey,
        visit: &mut dyn for<'a> FnMut(Record<'a>),
    ) -> bool {
        self.visit_checked(key, false, visit)
    }

    fn visit_checked(
        &self,
        key: DeclKey,
        indexed: bool,
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
                if indexed {
                    check_indexed_name(owner, image, key, record)?;
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
    let (record, _) = super::registrations::host_decode_prefix(
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
    use super::declaration_index::{key as name_key, record_hash, same_record_name};
    let kind = Kind::of(&record).ok_or(ERR_INVALID)?;
    let (category, _) = name_key(record).ok_or(ERR_INVALID)?;
    let hash = record_hash(record).ok_or(ERR_INVALID)?;
    let slots = owner.of_kind(kind);
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
    let peer_kinds = if category == 4 {
        &[Kind::Face, Kind::Extension][..]
    } else {
        core::slice::from_ref(&kind)
    };
    for peer_kind in peer_kinds {
        let peers = owner.of_kind(*peer_kind);
        let from = peers.partition_point(|slot| slot.hash < hash);
        for peer in peers[from..]
            .iter()
            .take_while(|slot| slot.hash == hash)
            .filter(|slot| slot.batch != key.batch || slot.record != key.record)
        {
            let Ok(other) = decode_entry(owner, image, peer.batch, peer.record) else {
                continue;
            };
            if name_key(other).is_some_and(|(other_kind, _)| {
                other_kind == category
                    && record_hash(other) == Some(hash)
                    && same_record_name(record, other)
            }) {
                return Err(ERR_INVALID);
            }
        }
    }
    Ok(())
}

#[path = "declaration_formats.rs"]
mod formats;
#[allow(
    unused_imports,
    reason = "read by the document service's production host"
)]
pub(crate) use formats::{FormatDecl, ReaderDecl};
#[path = "declaration_stages.rs"]
mod stages;
#[allow(
    unused_imports,
    reason = "read by the broker's adapter when slot 32 is wired"
)]
pub(crate) use stages::{StageLimits, StageSubscriber};

#[cfg(test)]
#[path = "declaration_store_tests.rs"]
mod tests;

// The device check validates layout instead of extrapolating host enum sizes.
#[cfg(target_pointer_width = "32")]
const _: () = {
    assert!(size_of::<DeclRef>() == 12);
    assert!(size_of::<DeclKey>() == 16);
    assert!(size_of::<Slot>() == 8);
    // One list and one table of offsets per owner, flat as kinds are added
    // (`abi/heap-word.md` section 9).
    assert!(size_of::<PendingDeclarations>() <= PENDING_BUDGET);
};

/// The most bytes of one owner's `PendingDeclarations` on the X4.
#[cfg(target_pointer_width = "32")]
const PENDING_BUDGET: usize = 192;
