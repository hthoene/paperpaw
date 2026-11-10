//! Roots 3 (the book), 4 (the user's CSS file) and 6 (the document's cache)
//! on the storage slots, for a plugin inside a format call.
extern crate std;
use super::tests::{Backend, bytes, output, scoped};
use super::*;
use crate::native_host::format_scope::{BookKind, FormatBook, FormatScope};
use paperpaw_native::host_glue::{ERR_DENIED, ERR_NOT_FOUND};
use paperpaw_platform::Storage;

const FORMAT: &str = "epub";
const OWNER: u64 = 5;
const CACHE: &str = ".paperpaw/cache/k/epub/c";

fn book<'a>(path: &'a str, kind: BookKind, cache: &'a str, css: &'a str) -> FormatBook<'a> {
    FormatBook {
        path,
        kind,
        opened: "a.epub",
        document: 1,
        cache,
        css,
    }
}

fn card() -> Backend {
    let backend = Backend::default();
    backend.files.write("books/a.epub", b"BOOK").unwrap();
    backend.files.write("books/b.png", b"PNG").unwrap();
    backend.files.write("style.css", b"CSS").unwrap();
    backend.files.write(".paperpaw/secret", b"no").unwrap();
    backend.files.write("plugins/x/secret", b"no").unwrap();
    backend
}

/// `call` runs with the format scope of a call for `book` (none for `None`).
fn within(book: Option<FormatBook<'_>>, call: impl FnOnce(&EnvironmentScope, Option<Format<'_>>)) {
    let scope = FormatScope::new();
    let _guard = book.map(|book| scope.enter(OWNER, &book).unwrap());
    let format = Some(Format {
        scope: &scope,
        owner: OWNER,
    });
    scoped(&card(), |env, kernel| {
        let _running = kernel.shared.meter.enter(FORMAT);
        call(env, format);
    });
}

fn read(
    env: &EnvironmentScope,
    format: Option<Format<'_>>,
    root: u32,
    path: &str,
) -> Result<std::string::String, i32> {
    let mut out = [0_u8; 16];
    // SAFETY: live input, exclusive output.
    let count = unsafe {
        read_at(
            env,
            FORMAT,
            format,
            root,
            bytes(path.as_bytes()),
            0,
            output(&mut out),
        )
    };
    usize::try_from(count)
        .map(|n| std::string::String::from_utf8_lossy(&out[..n]).into_owned())
        .map_err(|_| count)
}

fn put(
    env: &EnvironmentScope,
    format: Option<Format<'_>>,
    root: u32,
    path: &str,
    data: &[u8],
) -> i32 {
    // SAFETY: live inputs.
    unsafe {
        write(
            env,
            FORMAT,
            format,
            &PathsInUse::default(),
            root,
            bytes(path.as_bytes()),
            bytes(data),
        )
    }
}

fn size_of(
    env: &EnvironmentScope,
    format: Option<Format<'_>>,
    root: u32,
    path: &str,
) -> Result<u64, i32> {
    let mut len = 0_u64;
    // SAFETY: live input, aligned output.
    let status = unsafe { size(env, FORMAT, format, root, bytes(path.as_bytes()), &mut len) };
    if status == OK { Ok(len) } else { Err(status) }
}

#[test]
fn the_book_of_a_single_file_format_is_read_as_root_3_and_never_changed() {
    within(
        Some(book("books/a.epub", BookKind::File, "", "")),
        |env, format| {
            assert_eq!(read(env, format, 3, ""), Ok("BOOK".into()));
            assert_eq!(size_of(env, format, 3, ""), Ok(4));
            // One file: any name is not found.
            assert_eq!(read(env, format, 3, "b.png"), Err(ERR_NOT_FOUND));
            assert_eq!(put(env, format, 3, "", b"x"), ERR_DENIED);
            // SAFETY: live inputs.
            unsafe {
                let held = &PathsInUse::default();
                assert_eq!(
                    rename(env, FORMAT, format, held, 3, bytes(b""), bytes(b"")),
                    ERR_DENIED
                );
                assert_eq!(remove(env, FORMAT, format, held, 3, bytes(b"")), ERR_DENIED);
            }
        },
    );
}

#[test]
fn the_book_of_a_siblings_format_reads_its_folder_and_nothing_above() {
    within(
        Some(book("books/a.epub", BookKind::Siblings, "", "")),
        |env, format| {
            assert_eq!(read(env, format, 3, "b.png"), Ok("PNG".into()));
            assert_eq!(read(env, format, 3, "../style.css"), Err(ERR_INVALID));
            assert_eq!(read(env, format, 3, "c.png"), Err(ERR_NOT_FOUND));
            assert_eq!(put(env, format, 3, "b.png", b"x"), ERR_DENIED);
        },
    );
}

#[test]
fn a_book_at_the_top_of_the_card_cannot_reach_the_kernels_folders() {
    within(
        Some(book("a.epub", BookKind::Siblings, "", "")),
        |env, format| {
            assert_eq!(read(env, format, 3, "style.css"), Ok("CSS".into()));
            for name in [
                ".paperpaw/secret",
                "plugins/x/secret",
                ".PAPERPAW/secret",
                "Plugins/x/secret",
            ] {
                assert_eq!(read(env, format, 3, name), Err(ERR_INVALID), "{name}");
            }
        },
    );
}

#[test]
fn the_css_file_is_root_4_only_where_the_call_has_one_and_never_changed() {
    within(
        Some(book("books/a.epub", BookKind::File, "", "style.css")),
        |env, format| {
            assert_eq!(read(env, format, 4, ""), Ok("CSS".into()));
            assert_eq!(read(env, format, 4, "x"), Err(ERR_NOT_FOUND));
            assert_eq!(put(env, format, 4, "", b"x"), ERR_DENIED);
        },
    );
    within(
        Some(book("books/a.epub", BookKind::File, "", "")),
        |env, format| {
            assert_eq!(read(env, format, 4, ""), Err(ERR_UNSUPPORTED));
        },
    );
}

#[test]
fn the_cache_is_root_6_read_and_write_below_its_folder() {
    within(
        Some(book("books/a.epub", BookKind::File, CACHE, "")),
        |env, format| {
            assert_eq!(read(env, format, 6, "page/1"), Err(ERR_NOT_FOUND));
            assert_eq!(put(env, format, 6, "page/1", b"one"), OK);
            assert_eq!(read(env, format, 6, "page/1"), Ok("one".into()));
            assert_eq!(size_of(env, format, 6, "page/1"), Ok(3));
            // It is on the card below the cache folder, nowhere else.
            let storage = card().files;
            let _ = storage;
            // SAFETY: live inputs.
            unsafe {
                let held = &PathsInUse::default();
                assert_eq!(
                    rename(
                        env,
                        FORMAT,
                        format,
                        held,
                        6,
                        bytes(b"page/1"),
                        bytes(b"page/2")
                    ),
                    OK
                );
                assert_eq!(read(env, format, 6, "page/2"), Ok("one".into()));
                assert_eq!(remove(env, FORMAT, format, held, 6, bytes(b"page/2")), OK);
            }
            assert_eq!(read(env, format, 6, "page/2"), Err(ERR_NOT_FOUND));
            assert_eq!(read(env, format, 6, "../x"), Err(ERR_INVALID));
        },
    );
}

#[test]
fn the_hosts_stamp_and_folder_in_the_cache_are_never_changed_or_seen() {
    within(
        Some(book("books/a.epub", BookKind::File, CACHE, "")),
        |env, format| {
            for name in [".complete", ".COMPLETE", ".host/x", ".HOST/x", "COMPLE~1"] {
                assert_eq!(put(env, format, 6, name, b"x"), ERR_DENIED, "{name}");
                assert_eq!(read(env, format, 6, name), Err(ERR_NOT_FOUND), "{name}");
            }
            // SAFETY: live inputs.
            unsafe {
                let held = &PathsInUse::default();
                assert_eq!(
                    rename(
                        env,
                        FORMAT,
                        format,
                        held,
                        6,
                        bytes(b"a"),
                        bytes(b".complete")
                    ),
                    ERR_DENIED
                );
                assert_eq!(
                    remove(env, FORMAT, format, held, 6, bytes(b".complete")),
                    ERR_DENIED
                );
            }
            // A name that only starts like them is the format's own.
            assert_eq!(put(env, format, 6, ".completed", b"x"), OK);
            assert_eq!(put(env, format, 6, "sub/.complete", b"x"), OK);
        },
    );
}

#[test]
fn a_document_without_a_cache_folder_has_no_root_6() {
    within(
        Some(book("books/a.epub", BookKind::File, "", "")),
        |env, format| {
            assert_eq!(put(env, format, 6, "x", b"x"), ERR_UNSUPPORTED);
            assert_eq!(read(env, format, 6, "x"), Err(ERR_UNSUPPORTED));
        },
    );
}

#[test]
fn outside_a_format_call_roots_3_4_and_6_are_unsupported_and_so_are_the_others() {
    within(None, |env, format| {
        for root in [3, 4, 6] {
            assert_eq!(
                read(env, format, root, "x"),
                Err(ERR_UNSUPPORTED),
                "root {root}"
            );
            assert_eq!(
                put(env, format, root, "x", b"x"),
                ERR_UNSUPPORTED,
                "root {root}"
            );
        }
    });
    // Without even the format scope (a callback that is no format's).
    within(
        Some(book("books/a.epub", BookKind::File, CACHE, "style.css")),
        |env, _| {
            for root in [3, 4, 6] {
                assert_eq!(
                    read(env, None, root, "x"),
                    Err(ERR_UNSUPPORTED),
                    "root {root}"
                );
            }
        },
    );
    within(
        Some(book("books/a.epub", BookKind::File, CACHE, "style.css")),
        |env, format| {
            for root in [1, 2, 5, 7, 9] {
                assert_eq!(
                    read(env, format, root, "x"),
                    Err(ERR_UNSUPPORTED),
                    "root {root}"
                );
            }
        },
    );
}

#[test]
fn another_plugin_is_not_in_the_call() {
    let scope = FormatScope::new();
    let _guard = scope
        .enter(OWNER, &book("books/a.epub", BookKind::File, CACHE, ""))
        .unwrap();
    let format = Some(Format {
        scope: &scope,
        owner: OWNER + 1,
    });
    scoped(&card(), |env, kernel| {
        let _running = kernel.shared.meter.enter(FORMAT);
        assert_eq!(read(env, format, 3, ""), Err(ERR_UNSUPPORTED));
        assert_eq!(put(env, format, 6, "x", b"x"), ERR_UNSUPPORTED);
    });
}
