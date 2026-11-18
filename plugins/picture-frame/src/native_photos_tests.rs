#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use alloc::vec;

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| String::from(*s)).collect()
}

#[test]
fn only_jpeg_and_png_files_that_are_not_hidden_are_photos() {
    for (name, dir, photo) in [
        ("a.jpg", false, true),
        ("A.JPEG", false, true),
        ("b.PnG", false, true),
        ("c.gif", false, false),
        (".hidden.jpg", false, false),
        ("folder.jpg", true, false),
        ("noext", false, false),
        ("jpg", false, false),
    ] {
        assert_eq!(is_photo(name, dir), photo, "{name}");
    }
}

#[test]
fn the_names_kept_do_not_depend_on_the_order_the_card_lists_them_in() {
    let all: Vec<String> = (0..MOST + 40)
        .map(|n| alloc::format!("photo{n:04}.jpg"))
        .collect();
    let mut forward = Found::default();
    let mut backward = Found::default();
    for name in &all {
        forward.add(name, false).unwrap();
    }
    for name in all.iter().rev() {
        backward.add(name, false).unwrap();
    }
    let (forward, backward) = (forward.finish(), backward.finish());
    assert_eq!(forward.len(), MOST);
    assert_eq!(forward, backward);
    assert_eq!(forward.first().map(String::as_str), Some("photo0000.jpg"));
}

#[test]
fn names_are_sorted_without_regard_to_case_and_kept_within_their_room() {
    let mut found = Found::default();
    for name in ["b.jpg", "A.jpg", "c.png", "a.jpg", "notes.txt"] {
        found.add(name, false).unwrap();
    }
    assert_eq!(found.finish(), names(&["A.jpg", "a.jpg", "b.jpg", "c.png"]));
    let mut long = Found::default();
    let name = |n: u32| alloc::format!("{}{n:03}.jpg", "x".repeat(200));
    for n in 0..200 {
        long.add(&name(n), false).unwrap();
    }
    let kept = long.finish();
    assert!(kept.iter().map(String::len).sum::<usize>() <= NAMES);
    assert_eq!(kept.first(), Some(&name(0)));
}

#[test]
fn a_photo_is_found_by_its_place_in_the_card_folder_then_the_cache() {
    let photos = Photos {
        folder: "photos/trip".into(),
        card: names(&["a.jpg", "b.png"]),
        cached: names(&["p00000001.jpg"]),
    };
    assert_eq!(photos.len(), 3);
    assert_eq!(
        photos.at(0),
        Some((Source::Card, "photos/trip/a.jpg".into()))
    );
    assert_eq!(
        photos.at(2),
        Some((Source::Cache, "cache/p00000001.jpg".into()))
    );
    assert_eq!(photos.at(3), None);
    let root = Photos {
        card: names(&["a.jpg"]),
        ..Photos::default()
    };
    assert_eq!(root.at(0), Some((Source::Card, "a.jpg".into())));
    assert!(Photos::default().is_empty());
    // A path a request cannot carry is no photo.
    let long = Photos {
        card: vec!["x".repeat(PATH_MAX + 1)],
        ..Photos::default()
    };
    assert_eq!(long.at(0), None);
}

#[test]
fn each_sleep_goes_on_and_the_day_picks_the_photo_of_the_day() {
    assert_eq!(choose(0, Change::Sleep, 5, 100), None);
    assert_eq!(choose(4, Change::Sleep, 0, 100), Some(0));
    assert_eq!(choose(4, Change::Sleep, 6, 100), Some(2));
    assert_eq!(choose(4, Change::Daily, 99, 100), Some(0));
    assert_eq!(choose(4, Change::Daily, 99, 101), Some(1));
    // Every photo comes up within `count` days.
    let seen: alloc::collections::BTreeSet<_> = (20_000..20_004)
        .filter_map(|day| choose(4, Change::Daily, 0, day))
        .collect();
    assert_eq!(seen.len(), 4);
    // No clock (day 0) is still a photo.
    assert_eq!(choose(3, Change::Daily, 0, 0), Some(0));
    assert_eq!(choose(3, Change::Daily, 0, -1), Some(2));
}

#[test]
fn the_settings_name_a_change_and_a_fit() {
    use crate::declarations::{CHANGE, FIT};
    assert_eq!(Change::of(CHANGE[0]), Change::Sleep);
    assert_eq!(Change::of(CHANGE[1]), Change::Daily);
    assert_eq!(Change::of(""), Change::Sleep);
    assert_eq!(Fit::of(FIT[0]), Fit::Auto);
    assert_eq!(Fit::of(FIT[1]), Fit::Whole);
    assert_eq!(Fit::of(FIT[2]), Fit::Fill);
    assert_eq!(Fit::of("x"), Fit::Auto);
}

#[test]
fn automatic_fit_fills_a_near_shape_and_keeps_a_far_one_whole() {
    let portrait = Rect::new(0, 0, 480, 800);
    // 3:4 photo on a 3:5 screen: near enough.
    assert!(Fit::Auto.fills((600, 800), portrait));
    // A landscape photo on a portrait screen stays whole.
    assert!(!Fit::Auto.fills((1600, 1000), portrait));
    // The same photo on a landscape screen fills.
    assert!(Fit::Auto.fills((1600, 1000), Rect::new(0, 0, 800, 480)));
    assert!(Fit::Fill.fills((1600, 1000), portrait));
    assert!(!Fit::Whole.fills((600, 800), portrait));
    assert!(!Fit::Auto.fills((0, 0), portrait));
}

#[test]
fn the_tone_curve_keeps_the_ends_in_range_and_turns_the_middle_by_the_contrast() {
    let flat = tone(0);
    assert!(flat.iter().enumerate().all(|(v, t)| usize::from(*t) == v));
    let sharp = tone(50);
    assert_eq!(sharp[128], 128);
    assert_eq!(sharp[0], 0);
    assert_eq!(sharp[255], 255);
    assert!(sharp[160] > 160 && sharp[96] < 96);
    let soft = tone(-50);
    assert!(soft[0] > 0 && soft[255] < 255);
    // Out of range is clamped.
    assert_eq!(tone(500), tone(50));
    assert!(sharp.windows(2).all(|pair| pair[0] <= pair[1]));
}
