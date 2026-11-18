//! Reading the declaration store in a given order without copying the rows:
//! each step scans the stored rows of some kinds for the smallest sort key
//! after the last one, so a list of n rows costs n scans and no allocation.
//! The store keeps rows by owner and kind, not by `order`; the lists that
//! must come ordered (setup steps, the declared rows of a menu) read through
//! here. A record is only ever decoded inside its visitor, and the visitors
//! never reenter the store (its rule), so callers copy what they need out of
//! a row before looking at another.
use crate::native_host::{
    declaration_store::{DeclKey, DeclarationStore, Kind},
    registrations::Record,
};

/// A sort key: the key of the row, then its place in the scan, so equal keys
/// keep the store's order and every row has a key of its own.
pub(super) type Sort = (i64, u32);

/// The row after `after` (the first one for `None`) among the rows of
/// `kinds` that `key_of` gives a key (`None`: not in the list), by key.
pub(super) fn next_after(
    store: &DeclarationStore,
    kinds: &[Kind],
    key_of: &dyn Fn(&Record<'_>) -> Option<i64>,
    after: Option<Sort>,
) -> Option<(Sort, DeclKey)> {
    let mut best: Option<(Sort, DeclKey)> = None;
    let mut place = 0_u32;
    for kind in kinds {
        store.keys(*kind, &mut |key| {
            let at = place;
            place = place.saturating_add(1);
            store.with_record(key, &mut |record| {
                let Some(first) = key_of(&record) else {
                    return;
                };
                let sort = (first, at);
                if after.is_some_and(|last| sort <= last) {
                    return;
                }
                if best.is_none_or(|(current, _)| sort < current) {
                    best = Some((sort, key));
                }
            });
        });
    }
    best
}

/// Visits the row `key` names with the plugin that declared it.
pub(super) fn with_plugin(
    store: &DeclarationStore,
    key: DeclKey,
    visit: &mut dyn FnMut(Record<'_>, &str) -> core::ops::ControlFlow<()>,
) -> core::ops::ControlFlow<()> {
    let plugin = store.plugin_of(key).unwrap_or("");
    let mut flow = core::ops::ControlFlow::Continue(());
    store.with_record(key, &mut |record| flow = visit(record, plugin));
    flow
}
