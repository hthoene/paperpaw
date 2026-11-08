//! What the native plugin reads of an item's end and of a notes file's start
//! (`item_props`, `head_of`) against the interface-6 readers (`read_props`,
//! `markdown::head`): equal on the same file, reading no more than the caps
//! of interface 6 (2,048 bytes of an item's end, 1,024 of a notes file's
//! start), refusing instead of aborting when memory runs short.
use crate::fail_alloc::sweep;
use crate::native_joplin as native;
use crate::{item, markdown};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::MemoryStorage;

const PROPS: &str = "id: 0123456789abcdef0123456789abcdef\nparent_id: fedcba9876543210fedcba9876543210\ncreated_time: 2026-01-02T03:04:05.000Z\nupdated_time: 2026-02-03T04:05:06.000Z\nuser_created_time: 2026-01-02T03:04:05.000Z\norder: 5\ndeleted_time: 0\nencryption_applied: 0\ntype_: 1";

fn same_props(old: &item::Props, new: &native::Props) -> bool {
    (
        old.kind,
        &old.parent,
        &old.created,
        old.updated,
        &old.user_created,
    ) == (
        new.kind,
        &new.parent,
        &new.created,
        new.updated,
        &new.user_created,
    ) && (old.order, old.deleted, old.encrypted, old.text_end)
        == (new.order, new.deleted, new.encrypted, new.text_end)
}

#[test]
fn an_items_end_is_read_as_before() {
    assert_eq!(native::TAIL, 2048);
    assert_eq!(native::HEAD, 1024);
    let bodies = [
        String::new(),
        String::from("Title\n\nBody"),
        format!("Title\n\n{}", "text ünï\n\nmore ".repeat(400)),
        "x".repeat(2048 - PROPS.len() - 2),
        "x".repeat(2048 - PROPS.len() - 1),
        "y".repeat(5000),
    ];
    for body in &bodies {
        // With and without a trailing line break, and with a stray byte.
        for tail in ["", "\n", "\r\n\r\n"] {
            let file = format!("{body}\n\n{PROPS}{tail}");
            let storage = MemoryStorage::new().with("i.md", file.as_str());
            let old = item::read_props(&storage, "i.md").unwrap();
            // The whole file given: only its end counts.
            let new = native::item_props(file.len() as u64, file.as_bytes()).unwrap();
            assert!(same_props(&old, &new), "{} {tail:?}", file.len());
            // Just the end, as the caller reads it.
            let from = file.len().saturating_sub(native::TAIL);
            let new = native::item_props(file.len() as u64, &file.as_bytes()[from..]).unwrap();
            assert!(same_props(&old, &new), "{} {tail:?}", file.len());
        }
    }
    let mut bytes: Vec<u8> = b"T\n\n".to_vec();
    bytes.extend_from_slice(PROPS.as_bytes());
    bytes.insert(10, 0xff);
    let storage = MemoryStorage::new().with("i.md", bytes.clone());
    let old = item::read_props(&storage, "i.md").unwrap();
    assert!(same_props(
        &old,
        &native::item_props(bytes.len() as u64, &bytes).unwrap()
    ));
    // Properties beyond the end read are not seen, as before.
    let far = format!("{PROPS}\n\n{}", "z".repeat(3000));
    let storage = MemoryStorage::new().with("i.md", far.as_str());
    let old = item::read_props(&storage, "i.md").unwrap();
    let new = native::item_props(far.len() as u64, far.as_bytes()).unwrap();
    assert!(same_props(&old, &new));
    assert_eq!(new.kind, 0);
}

#[test]
fn the_start_of_notes_is_read_as_before() {
    let long_title = "T".repeat(1500);
    let files = [
        String::new(),
        String::from("# A Book\nJane Author\n\n## Highlight\n"),
        String::from("\n\n# Late title\n\nAuthor\n"),
        format!("# {long_title}\nAuthor\n"),
        format!("# Title\n{}Author far down\n", "\n".repeat(1100)),
        String::from("no heading\n"),
    ];
    for file in &files {
        let storage = MemoryStorage::new().with("n.md", file.as_str());
        let old = markdown::head(&storage, "n.md").unwrap();
        let new = native::head_of(file.as_bytes()).unwrap();
        assert_eq!(
            (old.title, old.author),
            (new.title, new.author),
            "{:.40}",
            file
        );
    }
    let invalid = b"# T\xffitle\nAu\xc3thor\n";
    let storage = MemoryStorage::new().with("n.md", invalid.to_vec());
    let old = markdown::head(&storage, "n.md").unwrap();
    let new = native::head_of(invalid).unwrap();
    assert_eq!((old.title, old.author), (new.title, new.author));
}

#[test]
fn refused_allocations_end_in_no_memory() {
    let file = format!("Title\n\n{PROPS}\n");
    sweep(|| native::item_props(file.len() as u64, file.as_bytes()).map(|p| p.parent.len()));
    sweep(|| native::head_of(b"# A \xff Book\nJane Author\n").map(|h| h.title.len()));
}
