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
