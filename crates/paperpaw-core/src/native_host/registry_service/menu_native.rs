//! What the declaration store holds for menus: the rows natively declared
//! plugins put into menus (`menu.entry`, `menu.section`, `menu.hide`,
//! `menu.need`, `menu.describe`, `icon.spec`; `screens-input.md` section 1).
//! They stay in the store (see `native_install`), so op 12 reads them here
//! beside what the environment's `Menus` holds for code plugins and the host.
//! A lookup copies the little it returns onto the stack, and no record is
//! decoded inside another's visitor (the store's rule). What one op call asks
//! again and again (a menu's rows in order, whether an item has a need, a
//! hide or an icon, whether a name is a menu) is read from the store once and
//! kept for the call ([`Memo`], #393): a row-by-row scan of every record for
//! every row made a menu of n rows cost n scans of the whole store, read from
//! flash on the X4. Without memory for it, every lookup scans as before.
use super::{
    fixed::{Fixed, caps},
    ordered::{self, Sort},
    rules,
};
use crate::native_host::{
    declaration_store::{DeclKey, DeclarationStore, Kind},
    registrations::Record,
};
use alloc::vec::Vec;
use core::cell::RefCell;
use paperpaw_digest::fnv1a64;
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
    memo: RefCell<Memo>,
}

/// What [`Native`] read once for its op call: hashes of names, sorted (a
/// name not among them is not there; one among them is looked up as before,
/// so a collision costs a scan, never a wrong answer), and the rows of the
/// menu last asked for, in their order. `None`: not read yet; read without
/// memory for it, the lookup scans.
#[derive(Default)]
struct Memo {
    /// The menus some row names (`menu.entry`, `menu.section`, `menu.hide`).
    menus: Option<Vec<u32>>,
    /// The items with a `menu.need`.
    needs: Option<Vec<u32>>,
    /// The (menu, item) pairs of `menu.hide`.
    hides: Option<Vec<u32>>,
    /// The items with an `icon.spec`.
    icons: Option<Vec<u32>>,
    /// The menu whose rows `rows` holds, and its rows by sort key.
    order: Option<(u32, Vec<(Sort, DeclKey)>)>,
}

fn hash(parts: &[&str]) -> u32 {
    let mut state = 0_u64;
    for part in parts {
        state = fnv1a64(part.as_bytes()) ^ state.rotate_left(17);
    }
    (state ^ state >> 32) as u32
}

/// Whether `wanted` is among `hashes` (sorted).
fn among(hashes: &[u32], wanted: &[&str]) -> bool {
    hashes.binary_search(&hash(wanted)).is_ok()
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
        Self {
            store,
            caps,
            memo: RefCell::new(Memo::default()),
        }
    }

    /// The hashes `name` gives the records of `kinds` (`None` to leave one
    /// out), sorted; `None` without memory.
    fn hashes(
        &self,
        kinds: &[Kind],
        name: &dyn Fn(&Record<'_>, &mut dyn FnMut(&[&str])),
    ) -> Option<Vec<u32>> {
        let mut out = Vec::new();
        let mut failed = false;
        for kind in kinds {
            self.scan(*kind, &mut |record, _| {
                name(record, &mut |parts| {
                    if out.try_reserve(1).is_err() {
                        failed = true;
                        return;
                    }
                    out.push(hash(parts));
                });
            });
        }
        if failed {
            return None;
        }
        out.sort_unstable();
        out.dedup();
        Some(out)
    }

    /// Whether the memo `pick` of [`Memo`] (read with `read` the first time)
    /// may hold `wanted`: `false` only where it surely does not.
    fn may_hold(
        &self,
        pick: fn(&mut Memo) -> &mut Option<Vec<u32>>,
        read: &dyn Fn() -> Option<Vec<u32>>,
        wanted: &[&str],
    ) -> bool {
        let mut memo = self.memo.borrow_mut();
        let slot = pick(&mut memo);
        if slot.is_none() {
            *slot = read();
        }
        slot.as_deref().is_none_or(|hashes| among(hashes, wanted))
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
        let read = || {
            self.hashes(
                &[Kind::MenuEntry, Kind::MenuSection, Kind::MenuHide],
                &|record, add| match record {
                    Record::MenuEntry(row) => add(&[row.menu]),
                    Record::MenuSection(row) => add(&[row.menu]),
                    Record::MenuHide(row) => add(&[row.menu]),
                    _ => {}
                },
            )
        };
        if !self.may_hold(|memo| &mut memo.menus, &read, &[menu]) {
            return false;
        }
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
        let (sort, key) = self.next_key(menu, after)?;
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

    /// The row after `after` in `menu`'s order, as [`ordered::next_after`]
    /// finds it: from the rows read once for the menu where there was memory.
    fn next_key(&self, menu: &str, after: Option<Sort>) -> Option<(Sort, DeclKey)> {
        let wanted = hash(&[menu]);
        let mut memo = self.memo.borrow_mut();
        if memo.order.as_ref().is_none_or(|(of, _)| *of != wanted) {
            memo.order = self.rows_of(menu).map(|rows| (wanted, rows));
        }
        let Some((_, rows)) = memo.order.as_ref() else {
            drop(memo);
            let key_of = |record: &Record<'_>| sort_key(menu, record);
            return ordered::next_after(
                self.store,
                &[Kind::MenuEntry, Kind::MenuSection],
                &key_of,
                after,
            );
        };
        let at = after.map_or(0, |last| rows.partition_point(|(sort, _)| *sort <= last));
        rows.get(at).copied()
    }

    /// `menu`'s declared rows with their sort keys, sorted: the keys
    /// [`ordered::next_after`] gives them (the same places, counted over both
    /// kinds); `None` without memory.
    fn rows_of(&self, menu: &str) -> Option<Vec<(Sort, DeclKey)>> {
        let mut rows = Vec::new();
        let (mut place, mut failed) = (0_u32, false);
        for kind in [Kind::MenuEntry, Kind::MenuSection] {
            self.store.keys(kind, &mut |key| {
                let at = place;
                place = place.saturating_add(1);
                self.store.with_record(key, &mut |record| {
                    let Some(first) = sort_key(menu, &record) else {
                        return;
                    };
                    if rows.try_reserve(1).is_err() {
                        failed = true;
                        return;
                    }
                    rows.push(((first, at), key));
                });
            });
        }
        if failed {
            return None;
        }
        rows.sort_unstable_by_key(|(sort, _)| *sort);
        Some(rows)
    }

    /// The plugin whose `menu.hide` takes `item` out of `menu`.
    pub(super) fn hidden_by(&self, menu: &str, item: &str) -> Option<Fixed<64>> {
        let read = || {
            self.hashes(&[Kind::MenuHide], &|record, add| {
                if let Record::MenuHide(row) = record {
                    add(&[row.menu, "\0", row.item]);
                }
            })
        };
        if !self.may_hold(|memo| &mut memo.hides, &read, &[menu, "\0", item]) {
            return None;
        }
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
        let read = || {
            self.hashes(&[Kind::MenuNeed], &|record, add| {
                if let Record::MenuNeed(row) = record {
                    add(&[row.item]);
                }
            })
        };
        if !self.may_hold(|memo| &mut memo.needs, &read, &[item]) {
            return true;
        }
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
        let read = || {
            self.hashes(&[Kind::Icon], &|record, add| {
                if let Record::IconSpec(row) = record {
                    add(&[row.item]);
                }
            })
        };
        if !self.may_hold(|memo| &mut memo.icons, &read, &[item]) {
            return None;
        }
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
