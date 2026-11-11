//! The native formats' files are books of the library too (#243, #245): the
//! kernel's catalog walk and its book check (`library::is_book_path`) ask the
//! SDK's `Formats`, which knows only the interface 6 formats. The kernel adds
//! **one** stand-in `FormatSpec` there for all the formats native plugins
//! declared (`format.spec`, `docs/abi/document.md` section 1): its id
//! [`STAND_IN_ID`], no title, and every extension they declared, matched as
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
//! keeps its extensions as they were taken while it ran, so its books never
//! drop out of a walk (and their caches are not taken for left over). A sync
//! the heap cut short is made again at the next call, not at the next
//! generation, for the same reason.
//!
//! **Interface 6 first.** An extension an interface 6 format claims is left
//! out of the stand-ins: a later format wins an extension in `Formats`, and an
//! interface 6 reader must keep opening its books (and the walk reading their
//! details) while both systems run.
//!
//! **Memory** (#248: one stand-in per format held about 2.3 KiB at the X4's
//! start peak for ten formats): one `FormatSpec` in `Formats`, its id and
//! title static, its extensions copied (the declarations live in the store,
//! which is not `'static`), and here a run of extensions per plugin; all made
//! fallibly at a sync, each list reserved exactly. A check allocates nothing.
//! During a sync the kept extensions are copied before the old stand-in goes,
//! so a sync the heap refuses leaves the old one as it was.
use super::declaration_store::DeclarationStore;
use super::format_index::extensions_of;
use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::{FormatSpec, Formats, Id};

/// The id of the one stand-in (never a plugin's: `kernel` is no plugin id).
pub(crate) const STAND_IN_ID: &str = "kernel.native-formats";

/// The stand-in in `Formats`, the plugins its extensions came from, and the
/// generation it was made at.
#[derive(Debug, Default)]
pub(crate) struct NativeBookFormats {
    /// The registry generation of the last sync; 0 for none yet.
    synced: u32,
    /// Per plugin (its host owner key), how many of the stand-in's
    /// extensions are its, in their order there.
    runs: Vec<(u64, u16)>,
}

/// A new stand-in's extensions, and its runs per plugin.
type Gathered = (Vec<Cow<'static, str>>, Vec<(u64, u16)>);

/// The heap refused: the sync is made again at the next call.
#[derive(Debug)]
struct NoRoom;

fn copy(text: &str) -> Result<Cow<'static, str>, NoRoom> {
    let mut copied = String::new();
    copied.try_reserve_exact(text.len()).map_err(|_| NoRoom)?;
    copied.push_str(text);
    Ok(Cow::Owned(copied))
}

/// Whether an interface 6 format in `formats` (any but the stand-in) claims
/// `extension`. Nothing is allocated.
fn claimed(formats: &Formats, extension: &str) -> bool {
    formats
        .iter()
        .filter(|spec| spec.id.as_str() != STAND_IN_ID)
        .any(|spec| {
            spec.extensions
                .iter()
                .any(|e| e.eq_ignore_ascii_case(extension))
        })
}

/// The new stand-in's extensions and runs: those of plugins that stopped but
/// stay (copied from `old`), then those of the live ones that no interface 6
/// format claims and that are not in it yet.
fn gather(
    formats: &Formats,
    store: &DeclarationStore,
    old: (&[Cow<'static, str>], &[(u64, u16)]),
) -> Result<Gathered, NoRoom> {
    let kept = |owner: u64| store.owner_runs(owner) == Some(false);
    let (mut most, mut plugins) = (0_usize, 0_usize);
    for &(owner, count) in old.1 {
        if kept(owner) {
            most += usize::from(count);
            plugins += 1;
        }
    }
    store.each_format(&mut |format| {
        most += extensions_of(format.spec.extensions).count();
        plugins += 1;
    });
    let (mut extensions, mut runs) = (Vec::new(), Vec::new());
    extensions.try_reserve_exact(most).map_err(|_| NoRoom)?;
    runs.try_reserve_exact(plugins).map_err(|_| NoRoom)?;
    let mut at = 0_usize;
    for &(owner, count) in old.1 {
        let taken = old.0.get(at..at + usize::from(count)).unwrap_or_default();
        at += usize::from(count);
        if kept(owner) {
            for extension in taken {
                extensions.push(copy(extension)?);
            }
            runs.push((owner, count));
        }
    }
    let mut failed = false;
    store.each_format(&mut |format| {
        let mut count = 0_u16;
        for extension in extensions_of(format.spec.extensions) {
            let known = extensions.iter().any(|e| e.eq_ignore_ascii_case(extension));
            if known || count == u16::MAX || claimed(formats, extension) {
                continue;
            }
            match copy(extension) {
                Ok(copied) => {
                    extensions.push(copied);
                    count += 1;
                }
                Err(NoRoom) => failed = true,
            }
        }
        if count > 0 {
            // Within the room reserved: one a declaring plugin's format.
            runs.push((format.owner, count));
        }
    });
    if failed {
        return Err(NoRoom);
    }
    Ok((extensions, runs))
}

impl NativeBookFormats {
    /// Whether the format `id` of `Formats` is the native formats' stand-in:
    /// its books are read by the host's details job (`doc_details_job`).
    pub(crate) fn has(&self, id: &str) -> bool {
        id == STAND_IN_ID && !self.runs.is_empty()
    }

    /// Whether there is no stand-in (no native format).
    pub(crate) fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// Makes the stand-in of `store`'s formats in `formats` again where
    /// `generation` is not the one it was made at, or the last sync was cut
    /// short: one the heap refused is made again at the next call.
    pub(crate) fn sync(
        &mut self,
        formats: &mut Formats,
        store: &DeclarationStore,
        generation: u32,
    ) {
        if self.synced == generation {
            return;
        }
        let old: &[Cow<'static, str>] = formats
            .iter()
            .find(|spec| spec.id.as_str() == STAND_IN_ID)
            .map_or(&[], |spec| &spec.extensions);
        // The old stand-in leaves `Formats` only once the new one is whole.
        let Ok((extensions, runs)) = gather(formats, store, (old, &self.runs)) else {
            return;
        };
        let had_old = formats.iter().any(|spec| spec.id.as_str() == STAND_IN_ID);
        if !had_old && !extensions.is_empty() && formats.try_reserve_formats(1).is_err() {
            return;
        }
        formats.retain_formats(|spec| spec.id.as_str() != STAND_IN_ID);
        self.runs = runs;
        if !extensions.is_empty() {
            let spec = FormatSpec::new(
                Id::new_static(STAND_IN_ID),
                Cow::Borrowed(""),
                extensions,
                // Never opened here: `core.document` opens native documents.
                // "Not ready" tells the walk its details come later, from the
                // host's details job (`library::keep_native_details`), so it
                // marks nothing as read.
                Box::new(|_, _, _| Err(paperpaw_sdk::document::DocumentError::NotReady)),
            );
            // Within the room reserved, and its id is the kernel's own.
            let _added = formats.add(spec);
        }
        self.synced = generation;
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
