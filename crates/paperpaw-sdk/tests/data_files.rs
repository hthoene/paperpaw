//! Files the SDK keeps on the card, as version 1 of their formats wrote
//! them, stay readable (see `docs/data-files.md`).

use paperpaw_sdk::document::Locator;
use paperpaw_sdk::secrets::Secrets;
use paperpaw_sdk::{MemoryStorage, annotations};

#[test]
fn secrets() {
    let secrets = Secrets::decode(include_str!("../testdata/secrets.txt.v1"));
    assert_eq!(secrets.get("wifi.Home Net"), Some("pass wörd"));
    assert_eq!(
        secrets.get("kosync.key"),
        Some("5f4dcc3b5aa765d61d8327deb882cf99")
    );
    let wifi = secrets.iter().next().unwrap();
    assert_eq!(wifi.label, "Wi-Fi Home Net");
    assert!(wifi.changeable);
}

#[test]
fn annotations_with_and_without_times() {
    let storage = MemoryStorage::new().with(
        &annotations::file("books/a.epub"),
        include_str!("../testdata/annotations.txt.v1"),
    );
    let all = annotations::load(&storage, "books/a.epub");
    assert_eq!(all.len(), 2);
    let (h, b) = (&all[0], &all[1]);
    assert_eq!(h.mark.start, Locator::new(2, 100));
    assert_eq!(h.mark.end, Locator::new(2, 180));
    assert_eq!((h.percent, h.created), (12, 0));
    assert_eq!(
        (h.text.as_str(), h.note.as_str()),
        ("All happy families", "A note")
    );
    assert!(h.mark.noted);
    assert_eq!(b.mark.start, Locator::new(3, 0));
    assert_eq!((b.created, b.updated), (1_760_000_000, 1_760_000_500));
    assert_eq!(b.text, "Chapter 2");
    assert!(!b.mark.noted);
}
