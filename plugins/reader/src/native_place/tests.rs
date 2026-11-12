use super::*;

fn progress(lengths: &[u32]) -> Progress {
    Progress::new(lengths.to_vec())
}

#[test]
fn progress_is_the_text_before_over_all_the_text() {
    let p = progress(&[100, 300, 600]);
    assert_eq!(p.ppm(0, 0), 0);
    assert_eq!(p.ppm(0, 50), 50_000);
    assert_eq!(p.ppm(1, 0), 100_000);
    assert_eq!(p.ppm(2, 300), 700_000);
    assert_eq!(p.ppm(2, 600), 1_000_000);
}

#[test]
fn progress_never_passes_the_whole_and_has_none_without_text() {
    let p = progress(&[10, 10]);
    // An offset past the end, a section past the last.
    assert_eq!(p.ppm(1, 9_999), 1_000_000);
    assert_eq!(p.ppm(7, 0), 1_000_000);
    assert_eq!(progress(&[]).ppm(0, 5), 0);
    assert_eq!(progress(&[0, 0]).ppm(1, 0), 0);
}

#[test]
fn lengths_that_would_wrap_a_u32_sum_still_add_up() {
    let p = progress(&[u32::MAX, u32::MAX]);
    assert_eq!(p.ppm(1, 0), 500_000);
    assert_eq!(p.ppm(2, 0), 1_000_000);
}

#[test]
fn an_unknown_progress_keeps_the_stored_one() {
    // The screen's own progress wins where it has one; else the stored.
    let own: Option<Progress> = Some(progress(&[100, 100]));
    let none: Option<Progress> = None;
    let pick = |p: &Option<Progress>| p.as_ref().map_or(777_000, |p| p.ppm(1, 0));
    assert_eq!(pick(&own), 500_000);
    assert_eq!(pick(&none), 777_000);
    assert_eq!(Stored::default(), Stored { at: None, ppm: 0 });
}

#[test]
fn a_title_is_cut_at_a_character_boundary() {
    assert_eq!(cut_title("Short"), "Short");
    let long: alloc::string::String = core::iter::repeat_n('é', 200).collect();
    let cut = cut_title(&long);
    assert_eq!(cut.len(), 256);
    assert!(cut.chars().all(|c| c == 'é'));
    // A three-byte letter straddling the limit is left out whole.
    let mixed: alloc::string::String = core::iter::repeat_n('a', 255).chain(['€']).collect();
    assert_eq!(cut_title(&mixed).len(), 255);
}

#[test]
fn the_book_is_read_to_the_end_from_999_thousandths() {
    assert!(!reaches_end(998_999));
    assert!(reaches_end(999_000));
    assert!(reaches_end(1_000_000));
}

#[test]
fn a_book_without_a_title_is_reported_by_its_path() {
    assert_eq!(title_of("Dune", "books/dune.epub"), "Dune");
    assert_eq!(title_of("", "books/dune.epub"), "books/dune.epub");
    let long: alloc::string::String = core::iter::repeat_n('a', 400).collect();
    assert_eq!(title_of("", &long).len(), 256);
}

#[test]
fn a_finished_book_reports_the_whole_also_when_paged_back() {
    assert_eq!(report_ppm(true, 987_654), 1_000_000);
    assert_eq!(report_ppm(false, 987_654), 987_654);
}

#[test]
fn a_share_of_the_book_is_the_place_its_progress_is() {
    let book = progress(&[100, 0, 300]);
    assert_eq!(book.place_at(0), (0, 0));
    assert_eq!(book.place_at(125_000), (0, 50));
    // An empty section is passed over.
    assert_eq!(book.place_at(250_000), (2, 0));
    assert_eq!(book.place_at(500_000), (2, 100));
    assert_eq!(book.place_at(999_000), (2, 299));
    // Past the end: the last character.
    assert_eq!(book.place_at(u32::MAX), (2, 299));
    // And back: the progress of the place is the share asked for.
    for ppm in [0, 125_000, 250_000, 500_000, 750_000] {
        let (section, offset) = book.place_at(ppm);
        assert_eq!(book.ppm(section, offset), ppm);
    }
    // A book without text has its start.
    assert_eq!(progress(&[]).place_at(500_000), (0, 0));
    assert_eq!(progress(&[0, 0]).place_at(500_000), (1, 0));
}

#[test]
fn a_book_the_library_has_not_listed_yet_is_tried_again_unsaid() {
    // #257: the last book reopened at the start, before the catalog walk listed
    // it, was said to be unsaved for want of memory.
    assert_eq!(place_refused(Error::NotFound, false), Refused::Retry);
    assert_eq!(place_refused(Error::Busy, false), Refused::Retry);
    // Closing, nothing tries again: said, but not as a lack of memory.
    assert_eq!(
        place_refused(Error::NotFound, true),
        Refused::Say("reader.not-saved-plain")
    );
}

#[test]
fn only_a_lack_of_memory_says_the_memory_may_be_full() {
    for closing in [false, true] {
        assert_eq!(
            place_refused(Error::NoMemory, closing),
            Refused::Say("reader.not-saved")
        );
        assert_eq!(
            place_refused(Error::Full, closing),
            Refused::Say("reader.not-saved-full")
        );
        for error in [
            Error::Io,
            Error::State,
            Error::Denied,
            Error::Invalid,
            Error::NoSpace,
        ] {
            assert_eq!(
                place_refused(error, closing),
                Refused::Say("reader.not-saved-plain"),
                "{error:?}"
            );
        }
    }
}

#[test]
fn a_refused_mark_names_a_full_card_and_nothing_else() {
    assert_eq!(marks_refused(Error::Full), "reader.marks-not-saved-full");
    for error in [Error::NotFound, Error::Io, Error::State, Error::Busy] {
        assert_eq!(marks_refused(error), "reader.marks-not-saved", "{error:?}");
    }
}

#[test]
fn every_refusal_text_is_there_in_english_and_german() {
    let texts = [
        include_str!("../../i18n/en.ini"),
        include_str!("../../i18n/de.ini"),
    ];
    let keys = [
        "reader.not-saved",
        "reader.not-saved-full",
        "reader.not-saved-plain",
        "reader.marks-not-saved",
        "reader.marks-not-saved-full",
    ];
    for text in texts {
        for key in keys {
            let found = text.lines().find_map(|l| {
                let (k, v) = l.split_once('=')?;
                (k.trim() == key).then(|| v.trim())
            });
            let value = found.unwrap_or_else(|| panic!("{key} missing"));
            // Only the lack of memory says the memory may be full.
            let full =
                value.contains("may be full") || value.contains("Vielleicht ist der Speicher voll");
            assert_eq!(full, key == "reader.not-saved", "{key}: {value}");
        }
    }
}
