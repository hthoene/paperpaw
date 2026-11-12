//! What the declaration store holds for menus: the rows natively declared
//! plugins put into menus (`menu.entry`, `menu.section`, `menu.hide`,
//! `menu.need`, `menu.describe`, `icon.spec`; `screens-input.md` section 1).
//! They stay in the store (see `native_install`), so op 12 reads them here
//! beside what the environment's `Menus` holds for code plugins and the host.
//! Every lookup scans the stored rows and copies the little it returns onto
//! the stack: nothing is allocated, and no record is decoded inside another's
//! visitor (the store's rule).
use super::{
    fixed::{Fixed, caps},
    ordered::{self, Sort},
    rules,
};
use crate::native_host::{
    declaration_store::{DeclarationStore, Kind},
    registrations::Record,
};
use paperpaw_sdk::Capabilities;

/// A row natively declared into a menu: an entry (an item of some plugin) or
/// a section heading.
pub(super) struct Entry {
    /// The full item id; a section's is `<plugin>.<name>`.
    pub(super) id: Fixed<{ caps::ID }>,
    /// The plugin that declared it.
    pub(super) by: Fixed<64>,
    pub(super) order: i32,
    /// A heading, whose `title` is a message key of its plugin.
    pub(super) section: bool,
    pub(super) title: Fixed<{ caps::ID }>,
}

/// The distance between the groups of sort keys: a group's keys are its
/// number times this, plus twice the order (`i16`, so negative too) and 0 or 1.
const BY_ORDER: i64 = 1 << 40;

/// The sort keys below this are those placed first.
pub(super) const FIRST_END: i64 = BY_ORDER / 2;

/// The sort keys from [`FIRST_END`] below this are those by order; above them
/// are those placed last.
pub(super) const BY_ORDER_END: i64 = BY_ORDER + BY_ORDER / 2;

/// `menu.need` 3, the one need a user's own list overrides.
const NO_TOUCH: u32 = 3;

pub(super) struct Native<'a> {
    store: &'a DeclarationStore,
    caps: Capabilities,
}

/// The sort key of a row in `menu`: those placed first, then those by order (a
/// heading before the entries of the same order), then those placed last. A
/// place this host does not know is not shown; one that names an anchor
/// (after, before, last in a section) is placed by order here, the place the
/// contract gives an anchor that is not there.
fn sort_key(menu: &str, record: &Record<'_>) -> Option<i64> {
    let (group, order, entry) = match record {
        Record::MenuEntry(row) if row.menu == menu => match row.place {
            4 => (0, row.order, 1),
            place if place < 4 => (1, row.order, 1),
            5 => (2, row.order, 1),
            _ => return None,
        },
        Record::MenuSection(row) if row.menu == menu => (1, row.order, 0),
        _ => return None,
    };
    Some(group * BY_ORDER + i64::from(order) * 2 + entry)
}

impl<'a> Native<'a> {
    pub(super) fn new(store: &'a DeclarationStore, caps: Capabilities) -> Self {
        Self { store, caps }
    }

    /// Visits every stored row of `kind` with its plugin.
    fn scan(&self, kind: Kind, visit: &mut dyn FnMut(&Record<'_>, &str)) {
        self.store.keys(kind, &mut |key| {
            let _ignored = ordered::with_plugin(self.store, key, &mut |record, plugin| {
                visit(&record, plugin);
                core::ops::ControlFlow::Continue(())
            });
        });
    }

    /// Whether some plugin put a row into `menu`.
    pub(super) fn names_menu(&self, menu: &str) -> bool {
        let mut found = false;
        for kind in [Kind::MenuEntry, Kind::MenuSection, Kind::MenuHide] {
            self.scan(kind, &mut |record, _| {
                found |= match record {
                    Record::MenuEntry(row) => row.menu == menu,
                    Record::MenuSection(row) => row.menu == menu,
                    Record::MenuHide(row) => row.menu == menu,
                    _ => false,
                };
            });
        }
        found
    }

    /// The row after `after` in the order of `menu`'s declared rows.
    pub(super) fn next(&self, menu: &str, after: Option<Sort>) -> Option<(Sort, Entry)> {
        let key_of = |record: &Record<'_>| sort_key(menu, record);
        let (sort, key) = ordered::next_after(
            self.store,
            &[Kind::MenuEntry, Kind::MenuSection],
            &key_of,
            after,
        )?;
        let mut entry = None;
        let _ignored = ordered::with_plugin(self.store, key, &mut |record, plugin| {
            let by = Fixed::of(&[plugin]).unwrap_or_else(Fixed::new);
            entry = match record {
                Record::MenuEntry(row) => Fixed::of(&[row.item]).map(|id| Entry {
                    id,
                    by,
                    order: row.order,
                    section: false,
                    title: Fixed::new(),
                }),
                Record::MenuSection(row) => Fixed::of(&[plugin, ".", row.name]).and_then(|id| {
                    Some(Entry {
                        id,
                        by,
                        order: row.order,
                        section: true,
                        title: Fixed::of(&[row.title])?,
                    })
                }),
                _ => None,
            };
            core::ops::ControlFlow::Continue(())
        });
        // A row whose id does not fit is skipped: the next call goes on.
        Some((sort, entry.unwrap_or_else(skipped)))
    }

    /// The plugin whose `menu.hide` takes `item` out of `menu`.
    pub(super) fn hidden_by(&self, menu: &str, item: &str) -> Option<Fixed<64>> {
        let mut by = None;
        self.scan(Kind::MenuHide, &mut |record, plugin| {
            if let Record::MenuHide(row) = record
                && by.is_none()
                && row.menu == menu
                && row.item == item
            {
                by = Some(Fixed::of(&[plugin]).unwrap_or_else(Fixed::new));
            }
        });
        by
    }

    /// Whether the device meets the `menu.need` rows of `item`; `listed`: the
    /// user put it into their own list, which `no-touch` does not take out.
    pub(super) fn need_met(&self, item: &str, listed: bool) -> bool {
        let mut met = true;
        self.scan(Kind::MenuNeed, &mut |record, _| {
            if let Record::MenuNeed(row) = record
                && row.item == item
                && !(listed && row.need == NO_TOUCH)
            {
                met &= rules::need_met(row.need, row.capability, self.caps);
            }
        });
        met
    }

    /// The icon of `item`, the last declared.
    pub(super) fn icon(&self, item: &str) -> Option<Fixed<64>> {
        let mut icon = None;
        self.scan(Kind::Icon, &mut |record, _| {
            if let Record::IconSpec(row) = record
                && row.item == item
            {
                icon = Fixed::of(&[row.icon]);
            }
        });
        icon
    }

    /// The message key of the line under `item`, from the plugin that owns it.
    pub(super) fn describe(&self, item: &str) -> Option<Fixed<{ caps::ID }>> {
        let (plugin, name) = item.split_once('.')?;
        let mut key = None;
        self.store
            .find(plugin, Kind::MenuDescribe, name, &mut |record| {
                if let Record::MenuDescribe(row) = record {
                    key = Fixed::of(&[row.key]);
                }
            });
        key
    }

    /// The message key of the heading `item` (`<plugin>.<name>`), where it is one.
    pub(super) fn section_title(&self, item: &str) -> Option<Fixed<{ caps::ID }>> {
        let (plugin, name) = item.split_once('.')?;
        let mut title = None;
        self.store
            .find(plugin, Kind::MenuSection, name, &mut |record| {
                if let Record::MenuSection(row) = record {
                    title = Fixed::of(&[row.title]);
                }
            });
        title
    }

    /// Whether some plugin declared `item` into `menu` (an entry, or the
    /// heading of that id).
    pub(super) fn declares(&self, menu: &str, item: &str) -> bool {
        let mut found = false;
        self.scan(Kind::MenuEntry, &mut |record, _| {
            if let Record::MenuEntry(row) = record {
                found |= row.menu == menu && row.item == item;
            }
        });
        found
            || self.section_title(item).is_some() && {
                let mut in_menu = false;
                self.scan(Kind::MenuSection, &mut |record, plugin| {
                    if let Record::MenuSection(row) = record {
                        in_menu |= row.menu == menu
                            && item
                                .strip_prefix(plugin)
                                .and_then(|rest| rest.strip_prefix('.'))
                                == Some(row.name);
                    }
                });
                in_menu
            }
    }
}

/// An entry that cannot be shown: no id, so no row.
fn skipped() -> Entry {
    Entry {
        id: Fixed::new(),
        by: Fixed::new(),
        order: 0,
        section: false,
        title: Fixed::new(),
    }
}
