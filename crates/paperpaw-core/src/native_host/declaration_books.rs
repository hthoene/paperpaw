//! Book fields and badges, read from the declaration store
//! (`docs/abi/library.md` section 1): what the catalog and `core.library` ask
//! to know of a field (is `<plugin>.<name>` declared, by whom, is it kept in
//! the catalog) and of the badges a page of rows is asked for. They live and
//! die with their plugin's declarations: a plugin that stops or reloads takes
//! its fields and badges with it, and there is no second registry.
#![allow(
    dead_code,
    reason = "the library catalog's production host reads these when it is wired"
)]
use super::{DeclarationStore, Kind};
use crate::native_host::registrations::{BookBadgeSpec, BookFieldSpec, Record};

/// The most `book_field.spec` a plugin declares (`library.md` section 1; the
/// ninth refuses the batch, `ERR_INVALID`).
pub(crate) const FIELDS_PER_PLUGIN: usize = 8;
/// The most `book_field.spec` on the device; the batch that would make the
/// 65th is refused (`ERR_FULL`).
pub(crate) const FIELDS_DEVICE: usize = 64;
/// The most `book_badge.spec` a plugin declares (Kernel's ruling: each owner
/// is one `core.book-badge` stage call per page of rows); the fifth refuses
/// the batch, `ERR_INVALID`.
pub(crate) const BADGES_PER_PLUGIN: usize = 4;
/// The most `book_badge.spec` on the device; the batch that would make the
/// 17th is refused (`ERR_FULL`).
pub(crate) const BADGES_DEVICE: usize = 16;
/// `book_field.spec.flags` bit 0: a short value kept in the catalog.
const FIELD_IN_CATALOG: u32 = 1;

/// One declared book field of a live plugin.
pub(crate) struct BookFieldDecl<'a, 'b> {
    /// The declaring plugin's host owner id.
    pub(crate) owner: u64,
    /// The declaring plugin's id.
    pub(crate) plugin: &'a str,
    pub(crate) spec: &'b BookFieldSpec<'b>,
}

impl BookFieldDecl<'_, '_> {
    /// Whether the value is a short one kept in the catalog (at most 64
    /// bytes), else it is one of at most 1,024 bytes kept per book.
    pub(crate) fn in_catalog(&self) -> bool {
        self.spec.flags & FIELD_IN_CATALOG != 0
    }
}

/// One declared book badge of a live plugin.
pub(crate) struct BookBadgeDecl<'a, 'b> {
    /// The declaring plugin's host owner id.
    pub(crate) owner: u64,
    /// The declaring plugin's id.
    pub(crate) plugin: &'a str,
    pub(crate) spec: &'b BookBadgeSpec<'b>,
}

impl DeclarationStore {
    /// Every book field of every live plugin, the plugin loaded first first, a
    /// plugin's own in its index order. The visitor must not invoke plugins
    /// or reenter the store.
    pub(crate) fn each_book_field(&self, visit: &mut dyn FnMut(&BookFieldDecl<'_, '_>)) {
        self.each_in_load_order(Kind::BookField, &mut |owner, plugin, record| {
            if let Record::BookFieldSpec(spec) = record {
                visit(&BookFieldDecl {
                    owner,
                    plugin,
                    spec: &spec,
                });
            }
        });
    }

    /// Every book badge of every live plugin, in the same order (the order
    /// the views show them in: two at most).
    pub(crate) fn each_book_badge(&self, visit: &mut dyn FnMut(&BookBadgeDecl<'_, '_>)) {
        self.each_in_load_order(Kind::BookBadge, &mut |owner, plugin, record| {
            if let Record::BookBadgeSpec(spec) = record {
                visit(&BookBadgeDecl {
                    owner,
                    plugin,
                    spec: &spec,
                });
            }
        });
    }

    /// The book field `id` (`<plugin>.<name>`) of a live plugin: `Some(in_catalog)`
    /// when it is declared, `None` when it is not (or its plugin is gone).
    /// The owner that may write it is the plugin in `id`, as the host stamps
    /// the declaring plugin (`library.md` op 23: own fields only).
    pub(crate) fn book_field(&self, id: &str) -> Option<bool> {
        let mut from = 0;
        while let Some(dot) = id[from..].find('.') {
            let at = from + dot;
            let (plugin, name) = (&id[..at], &id[at + 1..]);
            from = at + 1;
            let mut found = None;
            self.find(plugin, Kind::BookField, name, &mut |record| {
                if let Record::BookFieldSpec(spec) = record {
                    found = Some(spec.flags & FIELD_IN_CATALOG != 0);
                }
            });
            if found.is_some() {
                return found;
            }
        }
        None
    }
}

#[cfg(test)]
#[path = "declaration_books_tests.rs"]
mod tests;
