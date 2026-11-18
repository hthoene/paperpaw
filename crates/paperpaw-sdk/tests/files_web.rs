//! Files and web helpers: reading lines as they stream (against a plain
//! model, any bytes), free names, copying and moving (into itself, onto
//! itself), sizes; links, percent encoding, safe file names from
//! addresses (no path traversal), HTTP dates and Base64.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::ops::ControlFlow;

use paperpaw_sdk::web::{
    base64, basic_auth, clean_name, encode, file_name, host, join_url, percent_decode,
};
use paperpaw_sdk::{MemoryStorage, Storage, StorageError, files};
use proptest::prelude::*;

fn lines_of(storage: &MemoryStorage, path: &str) -> Vec<String> {
    let mut lines = Vec::new();
    files::for_each_line(storage, path, &mut |line| {
        lines.push(line.to_string());
        ControlFlow::Continue(())
    })
    .unwrap();
    lines
}

/// The lines of `bytes` as `for_each_line` documents them, all at once.
fn model_lines(bytes: &[u8]) -> Vec<String> {
    let mut pieces: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
    // After a last line break there is no line.
    if pieces.last().is_some_and(|p| p.is_empty()) {
        pieces.pop();
    }
    pieces
        .into_iter()
        .map(|piece| {
            let mut line = piece[..piece.len().min(files::MAX_LINE)].to_vec();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            String::from_utf8_lossy(&line).into_owned()
        })
        .collect()
}

fn decode_base64(text: &str) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    for chunk in text.as_bytes().chunks(4) {
        let values: Vec<u32> = chunk
            .iter()
            .filter(|b| **b != b'=')
            .map(|b| ALPHABET.iter().position(|a| a == b).unwrap() as u32)
            .collect();
        let n = values.iter().fold(0u32, |n, v| n << 6 | v) << (6 * (4 - values.len()));
        for i in 0..values.len() - 1 {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]

    /// Lines read as they stream are the lines of the whole file: any
    /// bytes, any line ends, lines longer than the limit, pieces of
    /// characters at the reading buffer's edges.
    #[test]
    fn streamed_lines_are_the_file_lines(
        parts in proptest::collection::vec(
            prop_oneof![
                proptest::collection::vec(any::<u8>(), 0..40),
                "[a-zé日\r\n]{0,700}".prop_map(String::into_bytes),
                Just(b"\r\n".to_vec()),
                Just(vec![b'x'; files::MAX_LINE + 3]),
            ],
            0..6,
        )
    ) {
        let bytes = parts.concat();
        let storage = MemoryStorage::new().with("list.txt", bytes.clone());
        prop_assert_eq!(lines_of(&storage, "list.txt"), model_lines(&bytes));
    }

    /// Text without spaces, encoded and decoded, is itself again, and
    /// what is encoded is plain ASCII safe in a query.
    #[test]
    fn percent_encoding_round_trips(text in "[^ ]{0,30}") {
        let encoded = encode(&text);
        prop_assert!(encoded.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.~%+".contains(&b)));
        prop_assert_eq!(percent_decode(&encoded), text);
    }

    /// Decoding any text does not panic.
    #[test]
    fn any_text_is_percent_decoded(text in "[%0-9a-fA-FzZ]{0,12}|\\PC{0,12}") {
        let _ = percent_decode(&text);
    }

    /// A file name made from an address never leaves its folder: no
    /// separators, no characters a card refuses, no leading dot, not
    /// empty, at most 200 characters.
    #[test]
    fn file_names_from_addresses_are_safe(url in "(https?://)?[a-z.]{0,10}(/[%./\\\\a-zA-Z0-9:*?\"<>|-]{0,20}){0,4}([?#].{0,5})?") {
        let name = file_name(&url);
        prop_assert!(!name.is_empty());
        prop_assert!(name.chars().count() <= 200);
        prop_assert!(!name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']));
        prop_assert!(!name.starts_with('.'));
        prop_assert!(!name.chars().any(char::is_control));
        prop_assert!(name != ".." && name != ".");
    }

    /// Any name made clean is safe the same way, or none at all.
    #[test]
    fn clean_names_are_safe(name in "\\PC{0,40}|[./\\\\ \t\n]{0,8}", max in 0usize..30) {
        if let Some(clean) = clean_name(&name, max) {
            prop_assert!(!clean.is_empty() && clean.chars().count() <= max);
            prop_assert!(!clean.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']));
            prop_assert!(!clean.starts_with('.') && clean.trim() == clean);
            prop_assert!(!clean.chars().any(char::is_control));
            prop_assert!(!clean.contains("  "));
        }
    }

    /// Links resolve to addresses on a host without `.` or `..` left in
    /// their path; any link and base resolve without panic.
    #[test]
    fn links_resolve_without_dot_segments(
        base in "https://[a-z]{1,5}\\.org(/[a-z.]{0,4}){0,3}(\\?[a-z=]{0,4})?(#[a-z]{0,3})?",
        // (Not `//`, which names another host.)
        href in "/?(\\.\\./|\\./|[a-z]{1,3}/){0,5}[a-z.]{0,4}(\\?[a-z]{0,3})?(#[a-z]{0,3})?",
    ) {
        let joined = join_url(&base, &href);
        prop_assert!(joined.starts_with("https://"));
        prop_assert_eq!(host(&joined), host(&base));
        let path = joined["https://".len()..]
            .split(['?', '#'])
            .next()
            .unwrap();
        let path = &path[host(&base).len()..];
        prop_assert!(path.starts_with('/'), "{joined}");
        prop_assert!(!path.split('/').any(|s| s == ".." || s == "."), "{joined}");
    }

    #[test]
    fn any_link_resolves_without_panic(base in "\\PC{0,20}", href in "\\PC{0,20}") {
        let _ = join_url(&base, &href);
        let _ = host(&base);
    }

    /// Base64 is read back as the bytes written, in RFC 4648's alphabet
    /// with its padding.
    #[test]
    fn base64_round_trips(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
        let text = base64(&bytes);
        prop_assert_eq!(text.len(), bytes.len().div_ceil(3) * 4);
        prop_assert_eq!(decode_base64(&text), bytes);
    }


}

#[test]
fn line_reading_stops_when_asked_and_reports_missing_files() {
    let storage = MemoryStorage::new().with("a.txt", "1\n2\n3\n");
    let mut seen = 0;
    files::for_each_line(&storage, "a.txt", &mut |_| {
        seen += 1;
        if seen == 2 {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })
    .unwrap();
    assert_eq!(seen, 2);
    assert_eq!(
        lines_of(&MemoryStorage::new().with("e.txt", ""), "e.txt"),
        Vec::<String>::new()
    );
    assert_eq!(
        lines_of(&MemoryStorage::new().with("n.txt", "\n"), "n.txt"),
        [""]
    );
    assert_eq!(
        lines_of(&MemoryStorage::new().with("r.txt", "\r"), "r.txt"),
        [""]
    );
}

#[test]
fn free_names_count_up_before_the_extension() {
    let storage = MemoryStorage::new()
        .with("d/a.epub", "")
        .with("d/a (2).epub", "")
        .with("d/.hidden", "")
        .with("d/noext", "")
        .with("d/x.tar.gz", "")
        .with("d/folder/inner.txt", "");
    assert_eq!(files::free_path(&storage, "d", "a.epub"), "d/a (3).epub");
    assert_eq!(files::free_path(&storage, "d", ".hidden"), "d/.hidden (2)");
    assert_eq!(files::free_path(&storage, "d", "noext"), "d/noext (2)");
    assert_eq!(
        files::free_path(&storage, "d", "x.tar.gz"),
        "d/x.tar (2).gz"
    );
    // A folder takes the name too.
    assert_eq!(files::free_path(&storage, "d", "folder"), "d/folder (2)");
    assert_eq!(files::free_path(&storage, "", "new.txt"), "new.txt");
}

#[test]
fn copies_and_moves_stay_out_of_themselves() {
    let storage = MemoryStorage::new()
        .with("books/a.txt", "a")
        .with("books/sub/b.txt", "b");
    assert_eq!(
        files::move_to(&storage, "books", "books/sub/books"),
        Err(StorageError::InvalidPath)
    );
    // A sibling whose name starts the same is no part of it.
    files::copy(&storage, "books", "books2").unwrap();
    assert_eq!(storage.get("books2/sub/b.txt").unwrap(), b"b");
    files::move_to(&storage, "books2", "moved").unwrap();
    assert!(files::is_dir(&storage, "moved") && !files::is_dir(&storage, "books2"));
    // What is not there is not copied.
    assert!(files::copy(&storage, "none.txt", "x.txt").is_err());
    // Paths that leave the storage are refused.
    assert_eq!(
        files::copy(&storage, "books/a.txt", "../a.txt"),
        Err(StorageError::InvalidPath)
    );
}

#[test]
fn copies_of_larger_files_are_whole() {
    let data: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
    let storage = MemoryStorage::new().with("big.bin", data.clone());
    files::copy(&storage, "big.bin", "copy.bin").unwrap();
    assert_eq!(storage.get("copy.bin").unwrap(), data);
}

#[test]
fn listings_put_folders_first_and_ignore_case() {
    let storage = MemoryStorage::new()
        .with("d/b.txt", "")
        .with("d/A.txt", "")
        .with("d/c/x", "")
        .with("d/.h", "");
    let names: Vec<String> = files::list(&storage, "d", false)
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(names, ["c", "A.txt", "b.txt"]);
    assert!(files::list(&storage, "none", false).is_err());
    assert!(files::is_hidden("a/.b/c") && files::is_hidden(".a") && !files::is_hidden("a/b.c"));
    assert_eq!((files::parent("a"), files::name("a")), ("", "a"));
    assert_eq!((files::parent("a/b/"), files::name("a/b/")), ("a/b", ""));
    assert_eq!(files::join("", "x"), "x");
    assert_eq!(files::join("a", "x"), "a/x");
}

#[test]
fn sizes_as_people_read_them() {
    for (bytes, text) in [
        (0, "0 B"),
        (999, "999 B"),
        (1000, "1,0 KB"),
        (1023, "1,0 KB"),
        (1536, "1,5 KB"),
        (10 * 1024, "10 KB"),
        (5 * 1024 * 1024, "5,0 MB"),
        (3 * 1024 * 1024 * 1024, "3,0 GB"),
        (u64::MAX, "17179869184 GB"),
    ] {
        assert_eq!(files::size(bytes, ","), text, "{bytes}");
    }
}

#[test]
fn names_from_hostile_addresses() {
    for (url, name) in [
        ("https://a.org/../../etc/passwd", "passwd"),
        ("https://a.org/%2E%2E%2F%2E%2E%2Fetc%2Fpasswd", "etc passwd"),
        ("https://a.org/a%5Cb%00c.epub", "a bc.epub"),
        ("https://a.org/%E2%80%AE.epub", "\u{202e}.epub"),
        ("https://a.org/%ff%fe.epub", "\u{fffd}\u{fffd}.epub"),
        ("https://a.org/C:%5Cwin.ini", "C win.ini"),
        ("file:///", "download"),
        ("", "download"),
    ] {
        assert_eq!(file_name(url), name, "{url}");
    }
    assert_eq!(
        file_name(&format!("https://a.org/{}.epub", "x".repeat(500)))
            .chars()
            .count(),
        200
    );
}

#[test]
fn base64_and_basic_authorisation_as_rfc_4648_has_them() {
    for (bytes, text) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(base64(bytes.as_bytes()), text);
    }
    assert_eq!(basic_auth("", ""), "Basic Og==");
    assert_eq!(
        basic_auth("ä", "p:w"),
        format!("Basic {}", base64("ä:p:w".as_bytes()))
    );
}

/// Copying a file onto itself keeps it: the copy is not begun by making
/// the target, which is the source, empty.
#[test]
fn copying_a_file_onto_itself_keeps_it() {
    let storage = MemoryStorage::new().with("a.txt", "keep me");
    let _ = files::copy(&storage, "a.txt", "a.txt");
    assert_eq!(storage.get("a.txt").unwrap(), b"keep me");
}

#[test]
fn deleting_a_folder_takes_everything_in_it() {
    let storage = MemoryStorage::new()
        .with("d/a.txt", "")
        .with("d/e/f.txt", "")
        .with("dd.txt", "");
    files::delete(&storage, "d").unwrap();
    assert!(!storage.exists("d/e/f.txt") && !files::is_dir(&storage, "d"));
    assert!(storage.exists("dd.txt"));
}
