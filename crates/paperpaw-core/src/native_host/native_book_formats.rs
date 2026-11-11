//! The native formats' files are books of the library too (#243, #245): the
//! kernel's catalog walk and its book check (`library::is_book_path`) ask the
//! SDK's `Formats`, which knows only the interface 6 formats. This is what the
//! kernel sets there (`Formats::set_others`): the extensions native format plugins
//! declared (`format.spec`, `docs/abi/document.md` section 1), matched as the
//! format index matches them (after a dot, ASCII case ignored).
//!
//! It reads the declaration store, the one the format index is made of
//! ([`FormatIndex::sync`](super::format_index::FormatIndex)), so it is never
//! behind it: a walk that starts once the native plugins are admitted sees
//! every format they declared. A check borrows the declared extensions and
//! allocates nothing (a walk asks for every file of the library on the X4).
//! A store that is busy or gone answers "none" for that one check.
use super::declaration_store::DeclarationStore;
use super::format_index::{extensions_of, has_extension};
use super::native_declarations::DirectoryWeak;

/// The native formats, through the kernel's weak handle on the declarations.
pub(crate) struct NativeBookFormats(DirectoryWeak);

impl NativeBookFormats {
    pub(crate) fn new(directory: DirectoryWeak) -> Self {
        Self(directory)
    }

    /// For `Formats::set_others`: with a file name, whether a native format
    /// claims it by its extension; with none, whether there is any.
    pub(crate) fn ask(&self, name: Option<&str>) -> bool {
        match name {
            Some(name) => self.claims(name),
            None => self.any(),
        }
    }

    fn with_store(&self, ask: impl FnOnce(&DeclarationStore) -> bool) -> bool {
        let Ok(Some(directory)) = self.0.upgrade() else {
            return false;
        };
        let Ok(store) = directory.store.try_borrow() else {
            return false;
        };
        ask(&store)
    }
}

impl NativeBookFormats {
    fn any(&self) -> bool {
        self.with_store(|store| {
            let mut any = false;
            store.each_format(&mut |_| any = true);
            any
        })
    }

    fn claims(&self, name: &str) -> bool {
        self.with_store(|store| {
            let mut claimed = false;
            store.each_format(&mut |format| {
                claimed = claimed
                    || extensions_of(format.spec.extensions).any(|e| has_extension(name, e));
            });
            claimed
        })
    }
}

#[cfg(test)]
#[path = "native_book_formats_tests.rs"]
mod tests;
