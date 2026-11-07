//! Weak metadata indexes. Only DeclarationStore retains plugin owners strongly.
use super::{
    declaration_store::{Allocations, ImageOwner},
    registrations::{Record, host_decode_prefix},
};
use alloc::{
    rc::{Rc, Weak},
    vec::Vec,
};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_NO_SPACE, ERR_STATE};

/// One checked bare-record range in a plugin's immutable metadata image.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct Span {
    offset: u32,
    len: u16,
    tag: u16,
}
impl Span {
    pub(crate) fn new(offset: usize, len: usize, tag: u32) -> Result<Self, i32> {
        let offset = u32::try_from(offset).map_err(|_| ERR_NO_SPACE)?;
        let len = u16::try_from(len).map_err(|_| ERR_NO_SPACE)?;
        let tag = u16::try_from(tag).map_err(|_| ERR_NO_SPACE)?;
        if len == 0 || offset.checked_add(u32::from(len)).is_none() {
            return Err(ERR_INVALID);
        }
        Ok(Self { offset, len, tag })
    }
}

/// The owner is an integer into the same update's weak owner table.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct Row {
    span: Span,
    owner: u16,
}
impl Row {
    pub(crate) fn new(owner: usize, span: Span) -> Result<Self, i32> {
        Ok(Self {
            span,
            owner: u16::try_from(owner).map_err(|_| ERR_NO_SPACE)?,
        })
    }
}
struct Owner {
    source: Weak<dyn ImageOwner>,
    generation: u32,
}
#[derive(Default)]
struct Data {
    owners: Vec<Owner>,
    rows: Vec<Row>,
    registry_order: Vec<u16>,
    scope_order: Vec<u16>,
}

/// Shared owns this catalog; it contains no dispatch, screens or Registry.
pub(crate) struct RegCatalog {
    generation: Cell<u32>,
    data: RefCell<Data>,
}
impl RegCatalog {
    pub(crate) fn new() -> Self {
        Self {
            generation: Cell::new(1),
            data: RefCell::new(Data::default()),
        }
    }
    pub(crate) fn generation(&self) -> u32 {
        self.generation.get()
    }

    /// Called only at kernel apply points with the store's current live owners.
    /// All reservations and validation precede publication. Rebuilding owners
    /// and rows together also removes entries whose store was dropped.
    pub(crate) fn replace(
        &self,
        owners: &[Rc<dyn ImageOwner>],
        rows: &[Row],
        allocations: &mut Allocations,
    ) -> Result<u32, i32> {
        let mut current = self.data.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        u16::try_from(owners.len()).map_err(|_| ERR_NO_SPACE)?;
        u16::try_from(rows.len()).map_err(|_| ERR_NO_SPACE)?;
        let generation = self.generation.get().checked_add(1).ok_or(ERR_NO_SPACE)?;
        let mut next = Data::default();
        allocations.reserve(&mut next.owners, owners.len())?;
        allocations.reserve(&mut next.rows, rows.len())?;
        allocations.reserve(&mut next.registry_order, rows.len())?;
        allocations.reserve(&mut next.scope_order, rows.len())?;
        next.owners.extend(owners.iter().map(|source| Owner {
            source: Rc::downgrade(source),
            generation: source.generation(),
        }));
        for (index, row) in rows.iter().enumerate() {
            visit_row(&next.owners, *row, &mut |_| Ok(()))?;
            let index = u16::try_from(index).map_err(|_| ERR_NO_SPACE)?;
            next.rows.push(*row);
            // Both orders begin with the producer's registry order. Scope
            // sorting uses only these integer indexes at the same apply point.
            next.registry_order.push(index);
            next.scope_order.push(index);
        }
        *current = next;
        self.generation.set(generation);
        Ok(generation)
    }

    /// The callback may encode into reply scratch, but cannot retain metadata
    /// borrows. Secret visibility belongs to the query's private predicate.
    pub(crate) fn with_record(
        &self,
        generation: u32,
        index: usize,
        visit: &mut dyn for<'a> FnMut(Record<'a>) -> Result<(), i32>,
    ) -> Result<(), i32> {
        let data = self.data.try_borrow().map_err(|_| ERR_BUSY)?;
        if generation != self.generation.get() {
            return Err(ERR_STATE);
        }
        let row = data.rows.get(index).ok_or(ERR_STATE)?;
        visit_row(&data.owners, *row, visit)
    }
}

fn visit_row(
    owners: &[Owner],
    row: Row,
    visit: &mut dyn for<'a> FnMut(Record<'a>) -> Result<(), i32>,
) -> Result<(), i32> {
    let owner = owners.get(usize::from(row.owner)).ok_or(ERR_STATE)?;
    let source = owner.source.upgrade().ok_or(ERR_STATE)?;
    if source.generation() != owner.generation {
        return Err(ERR_STATE);
    }
    let mut result = Err(ERR_STATE);
    source.with_image(&mut |image| {
        result = (|| {
            let image = image.ok_or(ERR_STATE)?;
            let offset = usize::try_from(row.span.offset).map_err(|_| ERR_STATE)?;
            let end = offset
                .checked_add(usize::from(row.span.len))
                .ok_or(ERR_STATE)?;
            let bytes = image.get(offset..end).ok_or(ERR_STATE)?;
            let (record, len) = host_decode_prefix(bytes).map_err(|_| ERR_STATE)?;
            if len != bytes.len() || tag(&record) != Some(u32::from(row.span.tag)) {
                return Err(ERR_STATE);
            }
            // Revalidate mutable native metadata before it reaches a reader.
            match super::declaration_index::key(record) {
                Some((8..=15, _)) => {
                    super::declaration_reg_checks::validate(source.plugin_id(), &record)
                }
                _ => super::declarations::validate_record(source.plugin_id(), &record),
            }
            .map_err(|_| ERR_STATE)?;
            visit(record)
        })();
    });
    result
}

fn tag(record: &Record<'_>) -> Option<u32> {
    use crate::abi7::records_wire as wire;
    Some(match record {
        Record::SettingGroup(_) => wire::SETTING_GROUP_TAG,
        Record::SettingSpec(_) => wire::SETTING_SPEC_TAG,
        Record::SettingExtra(_) => wire::SETTING_EXTRA_TAG,
        Record::ScreenSpec(_) => wire::SCREEN_SPEC_TAG,
        Record::ActionSpec(_) => wire::ACTION_SPEC_TAG,
        Record::SetupStep(_) => wire::SETUP_STEP_TAG,
        Record::SlotSpec(_) => wire::SLOT_SPEC_TAG,
        Record::SlotOffer(_) => wire::SLOT_OFFER_TAG,
        Record::KeyBind(_) => wire::KEY_BIND_TAG,
        Record::SettingDefault(_) => wire::SETTING_DEFAULT_TAG,
        Record::SettingChoice(_) => wire::SETTING_CHOICE_TAG,
        Record::PresetSpec(_) => wire::PRESET_SPEC_TAG,
        Record::PresetValue(_) => wire::PRESET_VALUE_TAG,
        _ => return None,
    })
}

#[cfg(test)]
#[path = "reg_catalog_tests.rs"]
mod tests;
