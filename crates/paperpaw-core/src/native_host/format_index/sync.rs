//! The index in line with the declaration store (`docs/abi/document.md`
//! section 1): the store holds what plugins declared and keeps it as long as
//! they live; the index adds what a reference needs that the store cannot
//! give, a [`FormatRef`] that is never reused. [`FormatIndex::sync`] is the one
//! place the two meet, run before a call reads the index.
use super::{Format, FormatIndex, FormatRef, Reader, joined, text};
use crate::native_host::{declaration_store::DeclarationStore, registrations as wire};
use alloc::vec::Vec;
use paperpaw_native::host_glue::ERR_NO_MEMORY;

/// Whether `format` is what the plugin `owner` declared as `decl`: the same
/// plugin, name and every field. A format whose fields changed is another one
/// (its documents end; a reference is never reused).
fn is(format: &Format, owner: u64, plugin: &str, spec: &wire::FormatSpec<'_>) -> bool {
    format.plugin == owner
        && format
            .id
            .strip_prefix(plugin)
            .and_then(|rest| rest.strip_prefix('.'))
            == Some(spec.name)
        && *format.title == *spec.title
        && *format.extensions == *spec.extensions
        && format.flags == spec.flags
        && format.priority == spec.priority
        && format.token == spec.token
}

impl FormatIndex {
    /// Make the index the store's: a format that is no longer declared (its
    /// plugin stopped, was removed or declares it differently now) goes, and
    /// `gone` hears its reference; a newly declared one is added in load
    /// order; the readers are replaced whole when they differ.
    ///
    /// Nothing the store holds is lost to a refusal: an allocation that fails
    /// is `ERR_NO_MEMORY` with the index as it was for the rest, and the next
    /// sync tries again. The generation moves only when a row did. Returns how
    /// many formats the index refused for another reason and skipped (the
    /// store admitted them under the same limits and names, so it is 0 unless
    /// the two disagree: the others are still listed, one format never hides
    /// the rest).
    ///
    /// # Errors
    /// `ERR_NO_MEMORY`.
    pub(crate) fn sync(
        &mut self,
        store: &DeclarationStore,
        mut gone: impl FnMut(FormatRef),
    ) -> Result<u32, i32> {
        self.retain_declared(store, &mut gone);
        let skipped = self.add_declared(store)?;
        self.replace_readers(store)?;
        Ok(skipped)
    }

    fn retain_declared(&mut self, store: &DeclarationStore, gone: &mut impl FnMut(FormatRef)) {
        let before = self.formats.len();
        self.formats.retain(|format| {
            let mut declared = false;
            store.each_format(&mut |f| {
                declared |= is(format, f.owner, f.plugin, f.spec);
            });
            if !declared {
                gone(format.reference);
            }
            declared
        });
        if self.formats.len() != before {
            self.changed();
        }
    }

    fn add_declared(&mut self, store: &DeclarationStore) -> Result<u32, i32> {
        let mut failure = None;
        let mut skipped = 0_u32;
        store.each_format(&mut |f| {
            if failure.is_some()
                || self
                    .formats
                    .iter()
                    .any(|x| is(x, f.owner, f.plugin, f.spec))
            {
                return;
            }
            match self.declare_format(f.owner, f.plugin, f.spec) {
                Ok(_) => {}
                // The heap: stop, the next sync resumes where this left off.
                Err(ERR_NO_MEMORY) => failure = Some(ERR_NO_MEMORY),
                // Anything else is the index disagreeing with the store's own
                // admission (same limits, same names, so it cannot happen
                // today): skip this one format, the rest stay listed.
                Err(_) => skipped = skipped.saturating_add(1),
            }
        });
        failure.map_or(Ok(skipped), Err)
    }

    /// The readers of the store, as the index lists them: `title` is the id of
    /// the plugin whose name the row shows.
    fn replace_readers(&mut self, store: &DeclarationStore) -> Result<(), i32> {
        let mut count = 0_usize;
        let mut same = true;
        store.each_reader(&mut |r| {
            same &= self.readers.get(count).is_some_and(|have| {
                have.id
                    .strip_prefix(r.plugin)
                    .and_then(|rest| rest.strip_prefix('.'))
                    == Some(r.spec.name)
                    && *have.screen == *r.spec.screen
                    && *have.formats == *r.spec.formats
                    && *have.title == *r.plugin
            });
            count += 1;
        });
        if same && count == self.readers.len() {
            return Ok(());
        }
        let mut fresh: Vec<Reader> = Vec::new();
        fresh.try_reserve_exact(count).map_err(|_| ERR_NO_MEMORY)?;
        let mut result = Ok(());
        store.each_reader(&mut |r| {
            if result.is_err() {
                return;
            }
            result = (|| -> Result<(), i32> {
                fresh.push(Reader {
                    id: joined(r.plugin, r.spec.name)?,
                    screen: text(r.spec.screen)?,
                    formats: text(r.spec.formats)?,
                    title: text(r.plugin)?,
                });
                Ok(())
            })();
        });
        result?;
        self.readers = fresh;
        self.changed();
        Ok(())
    }
}
