use super::*;
use crate::native_pages::Reading;

#[test]
fn the_handover_gives_the_place_params_and_nothing_else() {
    let found = Handover {
        path: "Books/a.epub".into(),
        root: Some("1".into()),
        section: Some("3".into()),
        offset: Some("40".into()),
        page: Some("7".into()),
    };
    assert_eq!(found.get("section"), Some("3"));
    assert_eq!(found.get("offset"), Some("40"));
    assert_eq!(found.get("page"), Some("7"));
    assert_eq!(found.get("path"), None);
    assert_eq!(Handover::default().get("section"), None);
}

#[test]
fn a_refusal_of_the_layout_is_the_hosts_where_it_has_one() {
    assert_eq!(
        document_error_kind(&DocumentError::OutOfMemory),
        Some(Error::NoMemory)
    );
    assert_eq!(
        document_error_kind(&DocumentError::NotReady),
        Some(Error::Busy)
    );
    assert_eq!(document_error_kind(&DocumentError::UnsupportedFormat), None);
}

#[test]
fn a_page_is_laid_out_again_when_the_reading_or_the_display_changes() {
    let reading = Reading::new(12, 120, 100, 4, 4, true);
    let key = LayoutKey {
        reading,
        display: (480, 800, 220),
        filtered: 0,
    };
    assert_eq!(key, key);
    let bigger = LayoutKey {
        reading: Reading::new(14, 120, 100, 4, 4, true),
        display: (480, 800, 220),
        filtered: 0,
    };
    assert_ne!(key, bigger);
    // A turned screen: the same reading, another size.
    let turned = LayoutKey {
        reading,
        display: (800, 480, 220),
        filtered: 0,
    };
    assert_ne!(key, turned);
    let justified = LayoutKey {
        reading: Reading::new(12, 120, 100, 4, 4, false),
        display: (480, 800, 220),
        filtered: 0,
    };
    assert_ne!(key, justified);
    // Other rules of the text filters (#257): other page breaks.
    let filtered = LayoutKey { filtered: 7, ..key };
    assert_ne!(key, filtered);
}

#[test]
fn a_file_a_plugin_hands_over_is_the_handed_file() {
    let handed = |root: Option<&str>| Handover {
        path: "own.txt".into(),
        root: root.map(Into::into),
        ..Handover::default()
    };
    // A maker's data (`0`) or install (`2`) folder, and the reader's own code.
    for root in ["0", "2", "5"] {
        assert_eq!(handed(Some(root)).root(), Root::Handed, "{root}");
    }
    // No root, the library's `1` and anything else: a book of the library.
    for root in [None, Some("1"), Some("3"), Some("")] {
        assert_eq!(handed(root).root(), Root::Library, "{root:?}");
    }
}

#[test]
fn the_root_of_a_hand_over_is_written_as_it_is_read() {
    assert_eq!(root_code(Root::Data), "0");
    assert_eq!(root_code(Root::Library), "1");
    assert_eq!(root_code(Root::Handed), "5");
}

#[test]
fn a_turn_past_the_last_page_stays_on_it() {
    use paperpaw_sdk::{document::ImageRef, layout::PageImage};
    // Past the book's end the format has nothing: the page laid there is empty.
    assert!(matches!(onto(Ok(Page::default())), Onto::Stay));
    // A page with only a picture on it is a page.
    let mut page = Page::default();
    let picture = ImageRef {
        path: "pictures/1.ppi".into(),
        width: 10,
        height: 10,
    };
    page.images
        .push(PageImage::new(0.0, 0.0, 10.0, 10.0, picture));
    assert!(matches!(onto(Ok(page)), Onto::Page(_)));
    // A layout that failed does not keep the reader on the page: the draw says why.
    assert!(matches!(onto(Err(Error::NoMemory)), Onto::Unlaid));
}
