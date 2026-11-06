//! Borrowed menu declarations encoded through the generated record codec.
use super::{Declarations, DeclareError, Item, Latched, filled, records_wire as wire, within};
use paperpaw_native::host_glue::ERR_INVALID;

/// Where a menu entry goes, with an anchor only for placements that need one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EntryPlacement<'a> {
    /// Sort by the declaration's order, lower first.
    Order,
    /// Follow this full entry id; use order if the anchor is absent.
    After(&'a str),
    /// Precede this full entry id; use order if the anchor is absent.
    Before(&'a str),
    /// Join this full section id; use order if the section is absent.
    InSection(&'a str),
    /// Place before other entries.
    First,
    /// Place after other entries.
    Last,
}

/// An action or setting offered in a menu, borrowing its declaration strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MenuEntryDecl<'a> {
    /// Full menu id, including the menu owner's namespace.
    pub menu: &'a str,
    /// Action or setting id relative to the declaring plugin.
    pub item: &'a str,
    /// Fallback order, lower first.
    pub order: i16,
    /// Placement relative to other entries.
    pub place: EntryPlacement<'a>,
}

impl<'a> MenuEntryDecl<'a> {
    /// Declare an entry sorted by order zero.
    #[must_use]
    pub const fn new(menu: &'a str, item: &'a str) -> Self {
        Self {
            menu,
            item,
            order: 0,
            place: EntryPlacement::Order,
        }
    }
}

impl<F: FnMut(&[u8]) -> Result<(), i32>> Declarations<F> {
    /// Offer an owned action or setting in a menu, which may belong to another
    /// plugin. Missing anchors fall back to order; the host resolves ids.
    ///
    /// # Errors
    /// Returns Empty or TooLong for fields outside the contract, Encode(Invalid)
    /// for malformed ids, or a latched builder or sink refusal.
    pub fn menu_entry(&mut self, entry: MenuEntryDecl<'_>) -> Result<&mut Self, DeclareError> {
        let record = (|| {
            menu_identifier(entry.menu, wire::MENU_ENTRY_MENU_MAX_BYTES, "menu")?;
            menu_identifier(entry.item, wire::MENU_ENTRY_ITEM_MAX_BYTES, "menu item")?;
            let (place, anchor) = match entry.place {
                EntryPlacement::Order => (0, ""),
                EntryPlacement::After(anchor) => (1, anchor),
                EntryPlacement::Before(anchor) => (2, anchor),
                EntryPlacement::InSection(anchor) => (3, anchor),
                EntryPlacement::First => (4, ""),
                EntryPlacement::Last => (5, ""),
            };
            if matches!(
                entry.place,
                EntryPlacement::After(_) | EntryPlacement::Before(_) | EntryPlacement::InSection(_)
            ) {
                menu_identifier(anchor, wire::MENU_ENTRY_ANCHOR_MAX_BYTES, "menu anchor")?;
            }
            Ok(Item::MenuEntry(wire::MenuEntry {
                menu: entry.menu,
                item: entry.item,
                order: i32::from(entry.order),
                place,
                anchor,
            }))
        })();
        self.accept_menu_declaration(record)
    }

    /// Give an owned action, screen or setting an icon name. Unknown names are
    /// accepted by the host and show no icon. No icon data is retained.
    ///
    /// # Errors
    /// Returns Empty or TooLong for fields outside the contract, Encode(Invalid)
    /// for malformed ids or icon names, or a latched builder or sink refusal.
    pub fn icon(&mut self, item: &str, icon: &str) -> Result<&mut Self, DeclareError> {
        let record = (|| {
            menu_identifier(item, wire::ICON_SPEC_ITEM_MAX_BYTES, "icon item")?;
            filled(icon, "icon name")?;
            within(icon, wire::ICON_SPEC_ICON_MAX_BYTES, "icon name")?;
            if !icon.bytes().all(menu_name_byte) {
                return Err(DeclareError::Encode(ERR_INVALID));
            }
            Ok(Item::Icon(wire::IconSpec { item, icon }))
        })();
        self.accept_menu_declaration(record)
    }

    /// Require a device feature for an owned action, setting or setting group.
    /// The host checks ownership and permits at most one need per item. An
    /// unknown capability is treated as unmet by a host that does not know it.
    ///
    /// # Errors
    /// Returns Empty or TooLong for the item id, Encode(Invalid) for malformed
    /// ids or an empty capability set, or a latched builder or sink refusal.
    pub fn menu_need(
        &mut self,
        item: &str,
        need: crate::MenuNeed,
    ) -> Result<&mut Self, DeclareError> {
        let record = (|| {
            menu_identifier(item, wire::MENU_NEED_ITEM_MAX_BYTES, "need item")?;
            let (need, capability) = match need {
                crate::MenuNeed::Speech => (0, 0),
                crate::MenuNeed::Sleep => (1, 0),
                crate::MenuNeed::Rotation => (2, 0),
                crate::MenuNeed::NoTouch => (3, 0),
                crate::MenuNeed::Touch => (4, 0),
                crate::MenuNeed::ButtonRow => (5, 0),
                crate::MenuNeed::Colors => (6, 0),
                crate::MenuNeed::Wifi => (7, 0),
                crate::MenuNeed::Firmware => (8, 0),
                crate::MenuNeed::BlackAndWhite => (9, 0),
                crate::MenuNeed::Frontlight => (10, 0),
                crate::MenuNeed::Bluetooth => (11, 0),
                crate::MenuNeed::Has(capability) => (12, capability.menu_declaration_bits()),
            };
            if need == 12 && capability == 0 {
                return Err(DeclareError::Encode(ERR_INVALID));
            }
            Ok(Item::MenuNeed(wire::MenuNeed {
                item,
                need,
                capability,
            }))
        })();
        self.accept_menu_declaration(record)
    }

    fn accept_menu_declaration(
        &mut self,
        record: Result<Item<'_>, DeclareError>,
    ) -> Result<&mut Self, DeclareError> {
        self.check_sink()?;
        if let Err(error) = record.and_then(|item| self.push(item)) {
            if self.failed.is_none() {
                self.failed = Some(Latched::Refused(error));
            }
            return Err(error);
        }
        Ok(self)
    }
}

fn menu_name_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
}

fn menu_identifier(value: &str, maximum: usize, field: &'static str) -> Result<(), DeclareError> {
    filled(value, field)?;
    within(value, maximum, field)?;
    if value
        .split('.')
        .any(|part| part.is_empty() || !part.bytes().all(menu_name_byte))
    {
        return Err(DeclareError::Encode(ERR_INVALID));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
