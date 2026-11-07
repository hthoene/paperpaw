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

/// An item offered in a menu, borrowing its declaration strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MenuEntryDecl<'a> {
    /// Full menu id, including the menu owner's namespace.
    pub menu: &'a str,
    /// Full item id, including its owner namespace; foreign items are allowed.
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

/// A plugin-owned heading contributed to a menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MenuSectionDecl<'a> {
    /// Section id relative to the declaring plugin.
    pub name: &'a str,
    /// Translation key of the heading label.
    pub title: &'a str,
    /// Full menu id, including its owner's namespace.
    pub menu: &'a str,
    /// Position among ordered entries, lower first.
    pub order: i16,
}

impl<'a> MenuSectionDecl<'a> {
    /// Declare a section sorted by order zero.
    #[must_use]
    pub const fn new(name: &'a str, title: &'a str, menu: &'a str) -> Self {
        Self {
            name,
            title,
            menu,
            order: 0,
        }
    }
}

impl<F: FnMut(&[u8]) -> Result<(), i32>> Declarations<F> {
    /// Offer an item in a menu, including another plugin's item or menu.
    /// Missing anchors fall back to order; the host resolves ids.
    ///
    /// # Errors
    /// Returns Empty or TooLong for fields outside the contract, Encode(Invalid)
    /// for malformed ids, or a latched builder or sink refusal.
    pub fn menu_entry(&mut self, entry: MenuEntryDecl<'_>) -> Result<&mut Self, DeclareError> {
        let record = (|| {
            menu_full_id(entry.menu, wire::MENU_ENTRY_MENU_MAX_BYTES, "menu")?;
            menu_full_id(entry.item, wire::MENU_ENTRY_ITEM_MAX_BYTES, "menu item")?;
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
                menu_full_id(anchor, wire::MENU_ENTRY_ANCHOR_MAX_BYTES, "menu anchor")?;
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

    /// Give any item an icon name. Unknown names are
    /// accepted by the host and show no icon. No icon data is retained.
    ///
    /// # Errors
    /// Returns Empty or TooLong for fields outside the contract, Encode(Invalid)
    /// for malformed ids or icon names, or a latched builder or sink refusal.
    pub fn icon(&mut self, item: &str, icon: &str) -> Result<&mut Self, DeclareError> {
        let record = (|| {
            menu_full_id(item, wire::ICON_SPEC_ITEM_MAX_BYTES, "icon item")?;
            filled(icon, "icon name")?;
            within(icon, wire::ICON_SPEC_ICON_MAX_BYTES, "icon name")?;
            if !icon.bytes().all(menu_name_byte) {
                return Err(DeclareError::Encode(ERR_INVALID));
            }
            Ok(Item::Icon(wire::IconSpec { item, icon }))
        })();
        self.accept_menu_declaration(record)
    }

    /// Require a device feature for an item, including a foreign item.
    /// The host permits at most one need per item in each submitted batch. An
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
            menu_full_id(item, wire::MENU_NEED_ITEM_MAX_BYTES, "need item")?;
            let (need, capability) = need_bits(need)?;
            Ok(Item::MenuNeed(wire::MenuNeed {
                item,
                need,
                capability,
            }))
        })();
        self.accept_menu_declaration(record)
    }

    /// Add an owned heading to any plugin's menu. The host resolves ids and
    /// checks section-name uniqueness at commit.
    ///
    /// # Errors
    /// Returns Empty or TooLong for fields outside the contract, Encode(Invalid)
    /// for malformed ids, or a latched builder or sink refusal.
    pub fn menu_section(
        &mut self,
        section: MenuSectionDecl<'_>,
    ) -> Result<&mut Self, DeclareError> {
        let record = (|| {
            menu_identifier(
                section.name,
                wire::MENU_SECTION_NAME_MAX_BYTES,
                "section name",
            )?;
            filled(section.title, "section title")?;
            within(
                section.title,
                wire::MENU_SECTION_TITLE_MAX_BYTES,
                "section title",
            )?;
            menu_full_id(
                section.menu,
                wire::MENU_SECTION_MENU_MAX_BYTES,
                "section menu",
            )?;
            Ok(Item::MenuSection(wire::MenuSection {
                name: section.name,
                title: section.title,
                menu: section.menu,
                order: i32::from(section.order),
            }))
        })();
        self.accept_menu_declaration(record)
    }

    /// Hide an item from a plugin-defined menu, including a foreign item.
    /// User-defined lists keep the item. The host checks duplicate pairs.
    ///
    /// # Errors
    /// Returns Empty or TooLong for fields outside the contract, Encode(Invalid)
    /// for malformed ids, or a latched builder or sink refusal.
    pub fn hide_menu_entry(&mut self, menu: &str, item: &str) -> Result<&mut Self, DeclareError> {
        let record = (|| {
            menu_full_id(menu, wire::MENU_HIDE_MENU_MAX_BYTES, "hidden menu")?;
            menu_full_id(item, wire::MENU_HIDE_ITEM_MAX_BYTES, "hidden item")?;
            Ok(Item::MenuHide(wire::MenuHide { menu, item }))
        })();
        self.accept_menu_declaration(record)
    }

    /// Add a translated description below one of the plugin's own items.
    /// The item id is relative; the host checks uniqueness at commit.
    ///
    /// # Errors
    /// Returns Empty or TooLong for fields outside the contract, Encode(Invalid)
    /// for a malformed item id, or a latched builder or sink refusal.
    pub fn describe_item(&mut self, item: &str, key: &str) -> Result<&mut Self, DeclareError> {
        let record = (|| {
            menu_identifier(item, wire::MENU_DESCRIBE_ITEM_MAX_BYTES, "described item")?;
            filled(key, "description key")?;
            within(key, wire::MENU_DESCRIBE_KEY_MAX_BYTES, "description key")?;
            Ok(Item::MenuDescribe(wire::MenuDescribe { item, key }))
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

pub(super) fn need_bits(need: crate::MenuNeed) -> Result<(u32, u64), DeclareError> {
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
    Ok((need, capability))
}

fn menu_name_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
}

pub(super) fn menu_identifier(
    value: &str,
    maximum: usize,
    field: &'static str,
) -> Result<(), DeclareError> {
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

pub(super) fn menu_full_id(
    value: &str,
    maximum: usize,
    field: &'static str,
) -> Result<(), DeclareError> {
    menu_identifier(value, maximum, field)?;
    if !value.contains('.') {
        return Err(DeclareError::Encode(ERR_INVALID));
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod more_tests;
