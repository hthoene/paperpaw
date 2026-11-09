//! Formats and reading apps, read from the declaration store
//! (`docs/abi/document.md` section 1): what the document service lists, selects
//! from and calls. A format and a reader live and die with their plugin's
//! declarations; there is no second registry of what is declared (the service's
//! own index keeps only what must outlive a plugin's reload: a format
//! reference is never reused).
#![allow(
    dead_code,
    reason = "the document service's production host reads these when it is wired"
)]
use super::{DeclarationStore, Kind};
use crate::native_host::registrations::{FormatSpec, ReaderSpec, Record};

/// One declared format of a live plugin.
pub(crate) struct FormatDecl<'a, 'b> {
    /// The declaring plugin's host owner id.
    pub(crate) owner: u64,
    /// The declaring plugin's id.
    pub(crate) plugin: &'a str,
    pub(crate) spec: &'b FormatSpec<'b>,
}

/// One declared reading app of a live plugin. A plugin declares at most 4 and
/// the device 16 (`document.md` section 1; checked at commit and in `prepare`).
pub(crate) struct ReaderDecl<'a, 'b> {
    pub(crate) plugin: &'a str,
    pub(crate) spec: &'b ReaderSpec<'b>,
}

impl DeclarationStore {
    /// Every format of every live plugin, the plugin loaded first first (its
    /// declarations' incarnation), a plugin's own in its index order. The
    /// visitor must not invoke plugins or reenter the store.
    pub(crate) fn each_format(&self, visit: &mut dyn FnMut(&FormatDecl<'_, '_>)) {
        self.each_in_load_order(Kind::Format, &mut |owner, plugin, record| {
            if let Record::FormatSpec(spec) = record {
                visit(&FormatDecl {
                    owner,
                    plugin,
                    spec: &spec,
                });
            }
        });
    }

    /// Every reading app of every live plugin, in the same order.
    pub(crate) fn each_reader(&self, visit: &mut dyn FnMut(&ReaderDecl<'_, '_>)) {
        self.each_in_load_order(Kind::Reader, &mut |_, plugin, record| {
            if let Record::ReaderSpec(spec) = record {
                visit(&ReaderDecl {
                    plugin,
                    spec: &spec,
                });
            }
        });
    }

    /// The records of `kind` of live plugins, by ascending incarnation: no
    /// allocation, a scan for the next larger incarnation per plugin.
    fn each_in_load_order(&self, kind: Kind, visit: &mut dyn for<'r> FnMut(u64, &str, Record<'r>)) {
        let mut last = 0;
        loop {
            let mut next: Option<(u32, usize)> = None;
            for (index, owner) in self.owners.iter().enumerate() {
                let Some(owner) = owner else {
                    continue;
                };
                if owner.source.generation() != owner.generation
                    || owner.incarnation <= last
                    || next.is_some_and(|(found, _)| found <= owner.incarnation)
                {
                    continue;
                }
                next = Some((owner.incarnation, index));
            }
            let Some((incarnation, index)) = next else {
                return;
            };
            last = incarnation;
            let Some(owner) = self.owners.get(index).and_then(Option::as_ref) else {
                continue;
            };
            let Ok(host) = owner.source.owner_id() else {
                continue;
            };
            for slot in owner.of_kind(kind) {
                let key = super::DeclKey {
                    owner: u16::try_from(index).unwrap_or(u16::MAX),
                    batch: slot.batch,
                    record: slot.record,
                    generation: owner.generation,
                    incarnation: owner.incarnation,
                };
                self.with_record(key, &mut |record| visit(host, owner.id.as_str(), record));
            }
        }
    }
}

#[cfg(test)]
#[path = "declaration_formats_tests.rs"]
mod tests;
