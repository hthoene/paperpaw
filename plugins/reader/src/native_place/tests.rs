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
