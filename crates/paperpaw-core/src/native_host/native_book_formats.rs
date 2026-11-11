//! The native formats' files are books of the library too (#243, #245): the
//! kernel's catalog walk and its book check (`library::is_book_path`) ask the
//! SDK's `Formats`, which knows only the interface 6 formats. The kernel adds
//! one stand-in `FormatSpec` there for each format a native plugin declared
//! (`format.spec`, `docs/abi/document.md` section 1): its id
//! `<plugin>.<name>`, its title and the extensions it declared, matched as
//! every format's are (after a dot, ASCII case ignored). Its opener refuses
//! with `NotReady`: a native document opens through `core.document`, never
//! `Formats::open`, and its details are read by the host later.
//!
//! **In line with the declarations.** The stand-ins are made again whenever
//! the registry's generation moved ([`Kernel::native_book_formats_sync`]):
//! the service pump's every pass, and before `core.catalog-wanted` is heard,
//! so the walk it starts sees every native format admitted by then. A plugin
//! removed, turned off or replaced takes its extensions with it; one that
//! only stopped (after a fault, or parked) and stays in the declaration store
//! keeps its stand-ins as they were made while it ran, so its books never
//! drop out of a walk (and their caches are not taken for left over). A sync
//! the heap cut short is made again at the next call, not at the next
//! generation, for the same reason.
//!
//! **Interface 6 first.** An extension an interface 6 format claims is left
//! out of the stand-ins: a later format wins an extension in `Formats`, and an
//! interface 6 reader must keep opening its books (and the walk reading their
//! details) while both systems run.
//!
//! **Memory.** A stand-in holds its id, title and extensions as copies (the
//! declarations live in the store, which is not `'static`), one allocation
//! each, made fallibly at a sync; a check allocates nothing. Per format:
//! the `FormatSpec` in `Formats` and its id here, plus the texts.
use super::declaration_store::DeclarationStore;
use super::format_index::extensions_of;
use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::{FormatSpec, Formats, Id};

/// The stand-ins in `Formats` and the generation they were made at.
#[derive(Debug, Default)]
pub(crate) struct NativeBookFormats {
    /// The registry generation of the last sync; 0 for none yet.
    synced: u32,
    /// The ids of the stand-ins added and their plugins' host owner keys, to
    /// take them out again.
    ids: Vec<(Id, u64)>,
}

/// The heap refused: the sync is made again at the next call.
#[derive(Debug)]
struct NoRoom;

fn copy(text: &str) -> Result<String, NoRoom> {
    let mut copied = String::new();
    copied.try_reserve_exact(text.len()).map_err(|_| NoRoom)?;
    copied.push_str(text);
    Ok(copied)
}

/// The stand-in for one declared format, with the extensions no format in
/// `formats` claims yet; `Ok(None)` where none is left (or the id is not a
/// valid one), [`NoRoom`] where the heap refuses.
fn stand_in(
    formats: &Formats,
    plugin: &str,
    name: &str,
    title: &str,
    list: &str,
) -> Result<Option<FormatSpec>, NoRoom> {
    let mut extensions = Vec::new();
    extensions
        .try_reserve_exact(extensions_of(list).count())
        .map_err(|_| NoRoom)?;
    let mut probe = String::new();
    for extension in extensions_of(list) {
        probe.clear();
        probe.try_reserve(extension.len() + 2).map_err(|_| NoRoom)?;
        probe.push_str("x.");
        probe.push_str(extension);
        if !formats.supports(&probe) {
            extensions.push(Cow::Owned(copy(extension)?));
        }
    }
    if extensions.is_empty() {
        return Ok(None);
    }
    let mut id = String::new();
    id.try_reserve_exact(plugin.len() + 1 + name.len())
        .map_err(|_| NoRoom)?;
    id.push_str(plugin);
    id.push('.');
    id.push_str(name);
    let id = match Id::try_parse(&id) {
        Ok(id) => id,
        Err(paperpaw_sdk::IdError::NoMemory) => return Err(NoRoom),
        // Not an id: never one, however often tried.
        Err(_) => return Ok(None),
    };
    Ok(Some(FormatSpec::new(
        id,
        Cow::Owned(copy(title)?),
        extensions,
        // Never opened here: `core.document` opens native documents. "Not
        // ready" tells the walk its details come later, from the host's
        // details job (`library::keep_native_details`), so it marks nothing
        // as read.
        Box::new(|_, _, _| Err(paperpaw_sdk::document::DocumentError::NotReady)),
    )))
}

impl NativeBookFormats {
    /// Makes the stand-ins of `store`'s formats in `formats` again where
    /// `generation` is not the one they were made at, or the last sync was
    /// cut short: one the heap refused is made again at the next call.
    pub(crate) fn sync(
        &mut self,
        formats: &mut Formats,
        store: &DeclarationStore,
        generation: u32,
    ) {
        if self.synced == generation {
            return;
        }
        // The stand-ins of a plugin that stopped but stays are kept (it cannot
        // be read while it does not run); the others are made again.
        let kept = |owner: u64| store.owner_runs(owner) == Some(false);
        let ids = &self.ids;
        formats.retain_formats(|spec| {
            !ids.iter()
                .any(|(id, owner)| *id == spec.id && !kept(*owner))
        });
        self.ids.retain(|(_, owner)| kept(*owner));
        let mut wanted = 0;
        store.each_format(&mut |_| wanted += 1);
        if self.ids.try_reserve_exact(wanted).is_err()
            || formats.try_reserve_formats(wanted).is_err()
        {
            // None now; tried again at the next call.
            return;
        }
        let mut whole = true;
        store.each_format(&mut |format| {
            let spec = format.spec;
            let stand_in = match stand_in(
                formats,
                format.plugin,
                spec.name,
                spec.title,
                spec.extensions,
            ) {
                Ok(Some(stand_in)) => stand_in,
                Ok(None) => return,
                Err(NoRoom) => {
                    whole = false;
                    return;
                }
            };
            let Some(id) = stand_in.id.try_clone() else {
                whole = false;
                return;
            };
            // A duplicate id (an interface 6 format of the same name) stays
            // the interface 6 one's.
            if formats.add(stand_in).is_ok() {
                // Within the room reserved: one a declared format.
                self.ids.push((id, format.owner));
            }
        });
        if whole {
            self.synced = generation;
        }
    }
}

impl crate::Kernel {
    /// The native formats' stand-ins in `Formats`, made again where the
    /// registry changed since (see the module). Nothing before the first
    /// native plugin; a store that is busy now is synced at the next call.
    pub(crate) fn native_book_formats_sync(&mut self) {
        let shared = &mut self.shared;
        let Some(directory) = shared.registry.native_declarations.as_ref() else {
            return;
        };
        let Ok(store) = directory.store.try_borrow() else {
            return;
        };
        let generation = shared.registry_generation.get();
        shared
            .registry
            .native_book_formats
            .sync(&mut shared.formats, &store, generation);
    }
}

#[cfg(test)]
#[path = "native_book_formats_tests.rs"]
pub(crate) mod tests;
