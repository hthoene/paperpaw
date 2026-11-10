//! The kernel's and the plugins' folders on the card are no book, no CSS file
//! and no file to sniff (root 1, `document.md` 1, `files.md` 3).
use super::*;
use paperpaw_native::host_glue::ERR_INVALID;

fn open_raw(
    w: &World<'_>,
    root: u32,
    path: &str,
    css_root: u32,
    css_path: &str,
) -> Result<Vec<u8>, i32> {
    w.call(
        AdmittedFeatures::test(1, FEATURES),
        1,
        &[Record::ServiceDocOpen(wire::ServiceDocOpen {
            root,
            path,
            format: "",
            flags: 0,
            image_side: 0,
            cover_side: 0,
            css_root,
            css_path,
        })],
    )
}

fn sniff_raw(w: &World<'_>, root: u32, path: &str) -> Result<Vec<u8>, i32> {
    w.call(
        AdmittedFeatures::test(1, FEATURES),
        20,
        &[Record::ServiceDocPath(wire::ServiceDocPath {
            root,
            path,
            flags: 1,
        })],
    )
}

#[test]
fn a_protected_name_on_the_card_is_no_book_in_any_spelling() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        for path in [
            "plugins/a.epub",
            "Plugins/a.epub",
            "PLUGINS/sub/a.epub",
            ".paperpaw/cache/a.epub",
            ".PaperPaw/a.epub",
            "paperpaw.ini",
            "PAPERPAW.INI",
            "Paperpaw.Ini",
        ] {
            assert_eq!(open_raw(w, 1, path, 0, ""), Err(ERR_INVALID), "{path}");
            assert_eq!(sniff_raw(w, 1, path), Err(ERR_INVALID), "sniff {path}");
        }
        assert!(w.calls.borrow().is_empty(), "no format was called");
        assert!(!w.hosts.in_use().holds("plugins/a.epub", false));
    });
}

#[test]
fn a_name_that_only_looks_alike_or_sits_deeper_or_in_root_0_is_a_book() {
    // One book a world: a document stays open and an opener holds few.
    for (root, path) in [
        (1, "books/paperpaw.ini.epub"),
        (1, "books/plugins/a.epub"),
        (1, "books/.paperpaw/a.epub"),
        (1, "plugins2/a.epub"),
        (1, "paperpaw.ini.epub"),
        (0, "plugins/a.epub"),
        (0, ".paperpaw/a.epub"),
    ] {
        world(|w| {
            w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
            assert!(open_raw(w, root, path, 0, "").is_ok(), "{root} {path}");
        });
    }
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        assert!(sniff_raw(w, 1, "books/x.epub").is_ok());
    });
}

#[test]
fn a_css_file_under_a_protected_name_is_refused_and_one_elsewhere_is_not() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        for css in [".paperpaw/cache/x.css", "Plugins/x.css", "paperpaw.ini"] {
            assert_eq!(open_raw(w, 0, "a.epub", 1, css), Err(ERR_INVALID), "{css}");
        }
        assert!(w.calls.borrow().is_empty());
    });
    // Root 0's own `.paperpaw` is not the card's; a CSS file elsewhere is fine.
    for (css_root, css) in [(0, ".paperpaw/x.css"), (1, "books/x.css")] {
        world(|w| {
            w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
            assert!(open_raw(w, 0, "a.epub", css_root, css).is_ok(), "{css}");
        });
    }
}
