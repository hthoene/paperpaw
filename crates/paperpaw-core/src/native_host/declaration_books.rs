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

/// Where a field or badge stands in the order `library.md` section 1 gives:
/// the owning plugin's id, then the `name`, both in byte order. A plugin id
/// and a name are at most 127 bytes together (`declarations::relative`).
struct Place {
    bytes: [u8; 128],
    plugin: usize,
    name: usize,
}

impl Place {
    fn of(plugin: &str, name: &str) -> Option<Self> {
        let mut bytes = [0; 128];
        let end = plugin.len().checked_add(name.len())?;
        bytes
            .get_mut(..plugin.len())?
            .copy_from_slice(plugin.as_bytes());
        bytes
            .get_mut(plugin.len()..end)?
            .copy_from_slice(name.as_bytes());
        Some(Self {
            bytes,
            plugin: plugin.len(),
            name: name.len(),
        })
    }

    fn key(&self) -> (&[u8], &[u8]) {
        (
            &self.bytes[..self.plugin],
            &self.bytes[self.plugin..self.plugin + self.name],
        )
    }
}

impl DeclarationStore {
    /// Every book field of every live plugin in the order of `library.md`
    /// section 1: by plugin id, then `name`, never by load order (a plugin
    /// that reloads keeps its place). The visitor must not invoke plugins or
    /// reenter the store.
    pub(crate) fn each_book_field(&self, visit: &mut dyn FnMut(&BookFieldDecl<'_, '_>)) {
        self.each_sorted(Kind::BookField, &mut |owner, plugin, record| {
            if let Record::BookFieldSpec(spec) = record {
                visit(&BookFieldDecl {
                    owner,
                    plugin,
                    spec: &spec,
                });
            }
        });
    }

    /// Every book badge of every live plugin, in the same order: the first
    /// two that answer for a book are the ones its row shows.
    pub(crate) fn each_book_badge(&self, visit: &mut dyn FnMut(&BookBadgeDecl<'_, '_>)) {
        self.each_sorted(Kind::BookBadge, &mut |owner, plugin, record| {
            if let Record::BookBadgeSpec(spec) = record {
                visit(&BookBadgeDecl {
                    owner,
                    plugin,
                    spec: &spec,
                });
            }
        });
    }

    /// The records of `kind` of live plugins by plugin id, then name: no
    /// allocation, a scan for the next larger place per record (a plugin
    /// declares few, the device 64 fields or 16 badges at most).
    fn each_sorted(&self, kind: Kind, visit: &mut dyn for<'r> FnMut(u64, &str, Record<'r>)) {
        let mut last: Option<Place> = None;
        loop {
            let mut next: Option<(Place, super::DeclKey, u64)> = None;
            for (index, owner) in self.owners.iter().enumerate() {
                let Some(owner) = owner else {
                    continue;
                };
                if owner.source.generation() != owner.generation {
                    continue;
                }
                let Ok(host) = owner.source.owner_id() else {
                    continue;
                };
                let _ = owner.places(kind, &mut |batch, record| {
                    let key = owner.key(index, batch, record);
                    self.with_record(key, &mut |record| {
                        let name = match record {
                            Record::BookFieldSpec(spec) => spec.name,
                            Record::BookBadgeSpec(spec) => spec.name,
                            _ => return,
                        };
                        let Some(place) = Place::of(owner.id.as_str(), name) else {
                            return;
                        };
                        if last.as_ref().is_some_and(|last| place.key() <= last.key())
                            || next
                                .as_ref()
                                .is_some_and(|(best, ..)| place.key() >= best.key())
                        {
                            return;
                        }
                        next = Some((place, key, host));
                    });
                    core::ops::ControlFlow::Continue(())
                });
            }
            let Some((place, key, host)) = next else {
                return;
            };
            let Some(owner) = self
                .owners
                .get(usize::from(key.owner))
                .and_then(Option::as_ref)
            else {
                return;
            };
            self.with_record(key, &mut |record| visit(host, owner.id.as_str(), record));
            last = Some(place);
        }
    }

    /// The book field `id` (`<plugin>.<name>`) of a live plugin: `Some(in_catalog)`
    /// when it is declared, `None` when it is not (or its plugin is gone).
    /// The owner that may write it is the plugin in `id`, as the host stamps
    /// the declaring plugin (`library.md` op 23: own fields only).
    pub(crate) fn book_field(&self, id: &str) -> Option<bool> {
        // A plugin id has no dot: the first one ends it.
        let (plugin, name) = id.split_once('.')?;
        let mut found = None;
        self.find(plugin, Kind::BookField, name, &mut |record| {
            if let Record::BookFieldSpec(spec) = record {
                found = Some(spec.flags & FIELD_IN_CATALOG != 0);
            }
        });
        found
    }
}

#[cfg(test)]
#[path = "declaration_books_tests.rs"]
mod tests;
