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
    registrations::{REGISTRATION_MAX_RECORDS, Record, host_records_visit},
    staged::DECLARATIONS_MAX_BYTES,
};
#[cfg(test)]
use alloc::rc::Rc;
use alloc::{boxed::Box, vec::Vec};
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

/// A native owner keeps its lease and host key inline: no box, and the plugin
/// id is kept once, in the owner's record. Test fixtures may use Rc; the
/// firmware has no such owner, so it pays no enum tag for one.
enum OwnerRef {
    #[cfg(test)]
    Existing(Rc<dyn ImageOwner>),
    Native {
        lease: super::native_instances::NativeLease,
        owner: u64,
    },
}
impl OwnerRef {
    fn try_clone(&self) -> Result<Self, i32> {
        match self {
            #[cfg(test)]
            Self::Existing(owner) => Ok(Self::Existing(Rc::clone(owner))),
            Self::Native { lease, owner } => Ok(Self::Native {
                lease: lease.try_clone()?,
                owner: *owner,
            }),
        }
    }
    /// A native owner is generation 1 while its instance runs, 0 after.
    fn generation(&self) -> u32 {
        match self {
            #[cfg(test)]
            Self::Existing(owner) => owner.generation(),
            Self::Native { lease, .. } => u32::from(lease.live().is_ok()),
        }
    }
    fn owner_id(&self) -> Result<u64, i32> {
        match self {
            #[cfg(test)]
            Self::Existing(owner) => owner.owner_id(),
            Self::Native { owner, .. } => Ok(*owner),
        }
    }
    fn native_lease(&self) -> Result<super::native_instances::NativeLease, i32> {
        match self {
            #[cfg(test)]
            Self::Existing(owner) => owner.native_lease(),
            Self::Native { lease, .. } => {
                lease.live()?;
                lease.try_clone()
            }
        }
    }
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        match self {
            #[cfg(test)]
            Self::Existing(owner) => owner.with_image(visit),
            Self::Native { lease, .. } => match lease.live() {
                Ok(instance) => instance.with_image(visit),
                Err(_) => visit(None),
            },
        }
    }
    fn invalid_declaration(&self, error: i32) {
        use super::native_screen::Dispatch as _;
        match self {
            #[cfg(test)]
            Self::Existing(owner) => owner.invalid_declaration(error),
            Self::Native { lease, .. } => lease.dropped(0, error),
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
    /// The record's place in its batch (0 the first), not a byte offset.
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

const KIND_COUNT: usize = 36;
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
    /// `book_field.spec` (category 34): a field the plugin keeps with books.
    BookField,
    /// `book_badge.spec` (category 35): a badge the plugin shows on book rows.
    BookBadge,
}
const _: () = assert!(Kind::BookBadge as usize + 1 == KIND_COUNT);

/// A slot's key keeps the kind in its top bits and the top of the name hash
/// in the others: one sorted list then holds every kind in its own range, with
/// no table of offsets per owner. Ten bits of hash make runs of equal keys
/// longer than full hashes would, and every lookup compares the names in a run
/// anyway: a slot is 4 bytes, not 8 (#248).
const KIND_SHIFT: u32 = 10;
const _: () = assert!(KIND_COUNT <= 1 << (16 - KIND_SHIFT));

impl Kind {
    /// The key of a record of this kind whose name hashes to `hash`: its top
    /// bits, so a kind keeps the order of the full hashes. Names whose hashes
    /// differ only in the low bits share a key: every lookup compares the
    /// names in a run of equal keys.
    fn slot_key(self, hash: u32) -> u16 {
        // Both parts fit: the kind below 2^6 (checked above), the hash part
        // below 2^10.
        ((self as u16) << KIND_SHIFT) | (hash >> (32 - KIND_SHIFT)) as u16
    }

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
            Self::BookField => 34,
            Self::BookBadge => 35,
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
            Record::BookFieldSpec(_) => Self::BookField,
            Record::BookBadgeSpec(_) => Self::BookBadge,
            _ => return None,
        })
    }
}

/// How many bits of a slot's place name the record in its batch: a batch
/// holds at most [`REGISTRATION_MAX_RECORDS`] records.
const RECORD_BITS: u32 = 3;
const _: () = assert!(REGISTRATION_MAX_RECORDS <= 1 << RECORD_BITS);
/// Most batches one owner keeps: the rest of a slot's place. Admission caps
/// stay below it (512 image entries; 32 KiB of owned bytes, each batch
/// charged its bytes and index), and a batch beyond is `ERR_NO_SPACE`.
const BATCHES_MAX: usize = 1 << (16 - RECORD_BITS);

#[derive(Clone, Copy)]
struct Slot {
    /// The kind and name hash (`Kind::slot_key`).
    hash: u16,
    /// The batch (top bits) and the record's place in it ([`RECORD_BITS`]).
    place: u16,
}

impl Slot {
    /// The batch, in the owner's order.
    fn batch(self) -> u16 {
        self.place >> RECORD_BITS
    }

    /// The record's place in its batch (0 the first).
    fn record(self) -> u16 {
        self.place & ((1 << RECORD_BITS) - 1)
    }
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

/// What the store keeps of one owner after admission.
struct StoredOwner {
    id: Id,
    generation: u32,
    source: OwnerRef,
    batches: Vec<DeclRef>,
    /// Every indexed record of the owner in one list, sorted by key: grouped
    /// by kind, and inside a kind by name hash.
    slots: Vec<Slot>,
    incarnation: u32,
}
impl StoredOwner {
    fn new(id: Id, source: OwnerRef) -> Self {
        Self {
            id,
            generation: source.generation(),
            source,
            batches: Vec::new(),
            slots: Vec::new(),
            incarnation: 0,
        }
    }

    /// The slots of one kind, sorted by name hash.
    fn of_kind(&self, kind: Kind) -> &[Slot] {
        let from = self
            .slots
            .partition_point(|slot| slot.hash < kind.slot_key(0));
        let rest = &self.slots[from..];
        let len = rest.partition_point(|slot| slot.hash >> KIND_SHIFT == kind as u16);
        &rest[..len]
    }

    /// The slots of `kind` whose name hashes to `hash` (`Kind::slot_key`).
    fn run(&self, kind: Kind, hash: u32) -> &[Slot] {
        let key = kind.slot_key(hash);
        let from = self.slots.partition_point(|slot| slot.hash < key);
        let rest = &self.slots[from..];
        &rest[..rest.partition_point(|slot| slot.hash == key)]
    }

    /// Adds `slot` after the slots whose key is not greater, so the list
    /// stays sorted. The caller has reserved room: nothing here allocates.
    fn add_slot(&mut self, slot: Slot) {
        let at = self.slots.partition_point(|old| old.hash <= slot.hash);
        self.slots.insert(at, slot);
    }
}

/// One owner during admission. The store keeps only `kept`: the charges
/// and the failure are dropped at commit.
pub(crate) struct PendingDeclarations {
    kept: StoredOwner,
    resident: usize,
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
        self.kept.id.as_str()
    }
    pub(crate) fn host_owner_id(&self) -> Result<u64, i32> {
        self.kept.source.owner_id()
    }

    /// The slots of one kind.
    fn of_kind(&self, kind: Kind) -> &[Slot] {
        self.kept.of_kind(kind)
    }

    fn with_source(id: Id, source: OwnerRef) -> Self {
        Self {
            kept: StoredOwner::new(id, source),
            resident: 0,
            failure: None,
            index_bytes: 0,
            image_entries: 0,
            owned_counts: [0; LIMITED_KINDS],
        }
    }

    #[cfg(test)]
    pub(crate) fn new(id: Id, source: Rc<dyn ImageOwner>) -> Self {
        Self::with_source(id, OwnerRef::Existing(source))
    }

    /// A native owner: `owner` is its host key, `lease` its running instance.
    pub(crate) fn native(id: Id, lease: super::native_instances::NativeLease, owner: u64) -> Self {
        Self::with_source(id, OwnerRef::Native { lease, owner })
    }

    /// Room for `batches` more batches and `entries` more indexed records,
    /// reserved once before they are staged one by one.
    pub(crate) fn reserve(
        &mut self,
        batches: usize,
        entries: usize,
        allocations: &mut Allocations,
    ) -> Result<(), i32> {
        allocations.reserve(&mut self.kept.batches, batches)?;
        allocations.reserve(&mut self.kept.slots, entries)
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
        let source = self.kept.source.try_clone()?;
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
        if self.kept.source.generation() != self.kept.generation {
            return Err(ERR_STATE);
        }
        let bytes = input.bytes();
        let (kinds, counts) = classify(self.kept.id.as_str(), bytes)?;
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
        let batch = self.kept.batches.len();
        if batch >= BATCHES_MAX {
            return Err(ERR_NO_SPACE);
        }
        let batch = u16::try_from(batch).map_err(|_| ERR_NO_SPACE)?;
        let mut reference = match &input {
            Input::Image { offset, len, .. } => Some(DeclRef::Image {
                offset: *offset,
                len: *len,
            }),
            _ => None,
        };
        if matches!(input, Input::Borrowed(_)) {
            self.kept.source.with_image(&mut |image| {
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
        allocations.reserve(&mut self.kept.batches, 1)?;
        allocations.reserve(&mut self.kept.slots, counts.iter().sum())?;
        for (record, kind) in kinds.into_iter().enumerate() {
            if let Some((kind, hash)) = kind {
                // The decoder bounds each batch to eight records
                // (`RECORD_BITS`), and `batch` is below `BATCHES_MAX`.
                self.kept.add_slot(Slot {
                    hash: kind.slot_key(hash),
                    place: batch << RECORD_BITS | record as u16,
                });
            }
        }
        self.kept.batches.push(reference);
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
                // And `format.spec`, `reader.spec`, `lookup.spec`, `book_field.spec`
                // and `book_badge.spec` (their counts are checked at commit).
                usize::MAX,
                usize::MAX,
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
    owners: Vec<Option<StoredOwner>>,
    incarnation: u32,
    /// Owners taken out so far.
    removed: u32,
}

/// Exclusive preparation prevents a second transaction invalidating reservations.
/// It holds only what the store keeps: the admission charges are gone.
pub(crate) struct Prepared<'a> {
    store: &'a mut DeclarationStore,
    owner: StoredOwner,
    at: usize,
}
impl Prepared<'_> {
    /// Every allocation and validation completed before this point.
    pub(crate) fn commit(self) {
        self.store.incarnation = self.owner.incarnation;
        if self.at == self.store.owners.len() {
            self.store.owners.push(Some(self.owner));
        } else {
            self.store.owners[self.at] = Some(self.owner);
        }
    }
}
impl DeclarationStore {
    pub(crate) fn prepare<'a>(
        &'a mut self,
        pending: PendingDeclarations,
        allocations: &mut Allocations,
    ) -> Result<Prepared<'a>, i32> {
        if let Some(error) = pending.failure {
            return Err(error);
        }
        let mut pending = pending.kept;
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
        // And 64 book fields, 16 book badges and 32 lookup providers on the
        // device (`library.md` section 1, `document.md` section 7), counted
        // the same way; the plugin's own caps are checked with its batch.
        for (kind, device, problem) in [
            (Kind::BookField, books::FIELDS_DEVICE, Cap::BOOK_FIELDS),
            (Kind::BookBadge, books::BADGES_DEVICE, Cap::BOOK_BADGES),
            (
                Kind::Lookup,
                super::declaration_checks::LOOKUPS_DEVICE,
                Cap::LOOKUPS,
            ),
        ] {
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
                .map(|owner| owner.of_kind(kind).len())
                .sum();
            if others + pending.of_kind(kind).len() > device {
                allocations.cap = Some(Cap::of(problem, device));
                return Err(paperpaw_native::host_glue::ERR_FULL);
            }
        }
        pending.incarnation = self.incarnation.checked_add(1).ok_or(ERR_NO_SPACE)?;
        Ok(Prepared {
            store: self,
            owner: pending,
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
            .map(|owner| owner.id.as_str())
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

    /// Every live record key, in owner and kind order.
    #[cfg(test)]
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
                    batch: slot.batch(),
                    record: slot.record(),
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
                    batch: slot.batch(),
                    record: slot.record(),
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
        for slot in owner.run(kind, hash) {
            let key = DeclKey {
                owner: u16::try_from(index).unwrap_or(u16::MAX),
                batch: slot.batch(),
                record: slot.record(),
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
    owner: &'a StoredOwner,
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
        super::raw_declarations::record_at(bytes, usize::from(record))?,
    )?;
    declarations::validate_record(owner.id.as_str(), &record)?;
    Ok(record)
}

fn check_indexed_name(
    owner: &StoredOwner,
    image: &[u8],
    key: DeclKey,
    record: Record<'_>,
) -> Result<(), i32> {
    use super::declaration_index::{key as name_key, record_hash, same_record_name};
    let kind = Kind::of(&record).ok_or(ERR_INVALID)?;
    let (category, _) = name_key(record).ok_or(ERR_INVALID)?;
    let hash = record_hash(record).ok_or(ERR_INVALID)?;
    if !owner
        .run(kind, hash)
        .iter()
        .any(|slot| slot.batch() == key.batch && slot.record() == key.record)
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
        for peer in owner
            .run(*peer_kind, hash)
            .iter()
            .filter(|slot| slot.batch() != key.batch || slot.record() != key.record)
        {
            let Ok(other) = decode_entry(owner, image, peer.batch(), peer.record()) else {
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

#[path = "declaration_books.rs"]
mod books;
#[path = "declaration_formats.rs"]
mod formats;
#[allow(
    unused_imports,
    reason = "read by the library catalog's production host"
)]
pub(crate) use books::{
    BADGES_DEVICE, BADGES_PER_PLUGIN, BookBadgeDecl, BookFieldDecl, FIELDS_DEVICE,
    FIELDS_PER_PLUGIN,
};
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

/// Each record's kind and hash by its place in the batch, `None` for a record
/// no kind indexes.
type Classes = [Option<(Kind, u32)>; REGISTRATION_MAX_RECORDS];

/// The kind and hash of each record of the batch `bytes` by its place, and how
/// many of each kind it holds: the whole batch decoded and checked first (each
/// record, then frames that tile it), with the statuses of a decoded batch.
/// One record is decoded at a time (`host_records_visit`), never the eight of a
/// batch at once: they would take most of the X4's 2 KiB stack band
/// (`abi/heap-word.md` section 6) under a plugin's own frames.
#[inline(never)]
fn classify(id: &str, bytes: &[u8]) -> Result<(Classes, [usize; KIND_COUNT]), i32> {
    let mut counts = [0usize; KIND_COUNT];
    let mut kinds = [None; REGISTRATION_MAX_RECORDS];
    let mut index = 0;
    // One visit checks and classifies each record: `host_records_visit` has
    // decoded the whole batch once before the first record is visited, so a
    // decoding error anywhere still comes before any record's own status.
    let len = host_records_visit(bytes, REGISTRATION_MAX_RECORDS, &mut |record| {
        declarations::validate_record(id, &record)?;
        if let Some(kind) = Kind::of(&record) {
            let hash = super::declaration_index::record_hash(record).ok_or(ERR_INVALID)?;
            *kinds.get_mut(index).ok_or(ERR_INVALID)? = Some((kind, hash));
            *counts.get_mut(kind as usize).ok_or(ERR_INVALID)? += 1;
        }
        index += 1;
        Ok(())
    })?;
    // The frames must tile the batch exactly: a lookup walks their headers
    // to reach a record by its place (`raw_declarations::record_at`).
    let framing = super::raw_declarations::offsets(bytes, len)?;
    debug_assert!(
        framing
            .get(..len)
            .is_some_and(|framing| framing.windows(2).all(|pair| pair[0] < pair[1]))
    );
    Ok((kinds, counts))
}

#[cfg(test)]
#[path = "declaration_store_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "declaration_classify_tests.rs"]
mod classify_tests;

// The device check validates layout instead of extrapolating host enum sizes.
#[cfg(target_pointer_width = "32")]
const _: () = {
    assert!(size_of::<DeclRef>() == 12);
    assert!(size_of::<DeclKey>() == 16);
    assert!(size_of::<Slot>() == 4);
    // One list per owner, with the kind in each slot's key: flat as kinds are
    // added (`abi/heap-word.md` section 9). The admission counts are not kept.
    assert!(size_of::<StoredOwner>() <= OWNER_BUDGET);
};

/// The most bytes the store keeps of one owner on the X4.
#[cfg(target_pointer_width = "32")]
const OWNER_BUDGET: usize = 64;
