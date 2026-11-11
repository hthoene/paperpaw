//! The native formats as the catalog's `Formats` asks them: by the extensions
//! declared now, as the format index matches them, and none once the plugin
//! stopped or the declarations are gone.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use super::*;
use crate::native_host::declaration_store::{Allocations, ImageOwner, PendingDeclarations};
use crate::native_host::native_declarations::NativeDeclarations;
use crate::native_host::registrations::{
    FormatSpec, REGISTRATION_MAX_BYTES, Record, records_encode,
};
use crate::native_host::shared::Shared;
use alloc::{rc::Rc, vec};
use core::cell::Cell;
use paperpaw_sdk::{Formats, Id};

struct Plugin {
    live: Cell<bool>,
}

impl ImageOwner for Plugin {
    fn plugin_id(&self) -> &str {
        "format-epub"
    }
    fn generation(&self) -> u32 {
        u32::from(self.live.get())
    }
    fn owner_id(&self) -> Result<u64, i32> {
        Ok(1)
    }
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        visit(self.live.get().then_some(&[][..]));
    }
    fn invalid_declaration(&self, _: i32) {}
}

/// The plugin declaring EPUB and FB2 in a ZIP.
fn declare(store: &mut DeclarationStore, plugin: &Rc<Plugin>) {
    let record = Record::FormatSpec(FormatSpec {
        name: "epub",
        title: "format.epub",
        extensions: "epub\nfb2.zip",
        flags: 0,
        priority: 0,
        token: 3,
    });
    let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
    let len = records_encode(&[record], &mut bytes).unwrap();
    let mut allocations = Allocations::default();
    let source: Rc<dyn ImageOwner> = plugin.clone();
    let mut pending = PendingDeclarations::new(Id::try_parse("format-epub").unwrap(), source);
    pending
        .stage_batch(&bytes[..len], &mut allocations)
        .unwrap();
    store.prepare(pending, &mut allocations).unwrap().commit();
}

#[test]
fn the_declared_extensions_are_books_and_none_once_the_plugin_stopped() {
    let directory = Shared::try_new(NativeDeclarations::default()).unwrap();
    let mut formats = Formats::new();
    formats.set_others(Box::new(NativeBookFormats::new(
        directory.downgrade().unwrap(),
    )));
    // Nothing declared yet: no book.
    assert!(formats.is_empty());
    assert!(!formats.supports("books/a.epub"));

    let plugin = Rc::new(Plugin {
        live: Cell::new(true),
    });
    declare(&mut directory.store.borrow_mut(), &plugin);
    // Read as declared now: no sync, nothing kept aside.
    assert!(!formats.is_empty());
    for book in ["books/a.epub", "A.EPUB", "deep/b.Fb2.Zip"] {
        assert!(formats.supports(book), "{book}");
    }
    for other in ["books/a.txt", "epub", "books/a.zip", "a.epubx"] {
        assert!(!formats.supports(other), "{other}");
    }

    // Busy store: none for that check, no panic.
    {
        let _busy = directory.store.borrow_mut();
        assert!(!formats.supports("books/a.epub"));
    }
    plugin.live.set(false);
    assert!(formats.is_empty());
    assert!(!formats.supports("books/a.epub"));
}

#[test]
fn declarations_that_are_gone_are_no_formats() {
    let directory = Shared::try_new(NativeDeclarations::default()).unwrap();
    let others = NativeBookFormats::new(directory.downgrade().unwrap());
    let plugin = Rc::new(Plugin {
        live: Cell::new(true),
    });
    declare(&mut directory.store.borrow_mut(), &plugin);
    assert!(others.other_format_claims("a.epub"));
    drop(directory);
    assert!(!others.any_other_format());
    assert!(!others.other_format_claims("a.epub"));
}
