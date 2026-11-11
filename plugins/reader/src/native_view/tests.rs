use super::*;

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
    };
    assert_eq!(key, key);
    let bigger = LayoutKey {
        reading: Reading::new(14, 120, 100, 4, 4, true),
        display: (480, 800, 220),
    };
    assert_ne!(key, bigger);
    // A turned screen: the same reading, another size.
    let turned = LayoutKey {
        reading,
        display: (800, 480, 220),
    };
    assert_ne!(key, turned);
    let justified = LayoutKey {
        reading: Reading::new(12, 120, 100, 4, 4, false),
        display: (480, 800, 220),
    };
    assert_ne!(key, justified);
}

#[test]
fn the_root_of_a_hand_over_is_written_as_it_is_read() {
    assert_eq!(root_code(Root::Data), "0");
    assert_eq!(root_code(Root::Library), "1");
    assert_eq!(root_code(Root::Handed), "5");
}
