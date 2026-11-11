use super::*;

fn action(action: &str) -> Input<'_> {
    Input::Action { now_ms: 0, action }
}

#[test]
fn settings_are_kept_to_what_they_allow() {
    let r = Reading::new(14, 130, 100, 6, 0, true);
    assert_eq!((r.margin, r.margin_ends), (6, 0));
    assert!((r.size_pt - 14.0).abs() < 1e-6);
    assert!((r.spacing_factor() - 1.3).abs() < 1e-6);
    // Not set (0), too small or too large: the default or the nearest allowed.
    let d = Reading::new(0, 0, 0, 0, 0, false);
    assert_eq!((d.line_spacing, d.word_spacing), (120, 100));
    assert!((d.size_pt - 12.0).abs() < 1e-6);
    // A margin of 0 mm is none, as in Beta 3.
    assert_eq!((d.margin, d.margin_ends), (0, 0));
    let wild = Reading::new(9_000, 10, 999, -4, 99, false);
    assert!((wild.size_pt - 72.0).abs() < 1e-6);
    assert_eq!((wild.line_spacing, wild.word_spacing), (80, 200));
    assert_eq!((wild.margin, wild.margin_ends), (0, 30));
    assert!((wild.word_factor() - 2.0).abs() < 1e-6);
}

#[test]
fn the_content_box_is_the_display_less_the_margins() {
    // 254 dpi: 1 mm is 10 px.
    let r = Reading::new(12, 120, 100, 5, 8, false);
    let b = content_box(480, 800, 254, &r).unwrap();
    assert_eq!((b.x, b.y, b.width, b.height), (50, 80, 380, 640));
    // Margins that leave nothing, and a display with no size.
    let wide = Reading::new(12, 120, 100, 30, 30, false);
    assert_eq!(content_box(500, 800, 254, &wide), None);
    assert_eq!(content_box(0, 0, 254, &r), None);
    assert_eq!(content_box(480, 800, 0, &r).map(|b| b.width), Some(480));
}

#[test]
fn a_status_line_keeps_the_text_from_its_edge() {
    let r = Reading::new(12, 120, 100, 5, 8, false);
    // Room above the margin moves the box; room within the margin changes nothing.
    let b = content_box(480, 800, 254, &r.with_status(100, 0)).unwrap();
    assert_eq!((b.y, b.height), (100, 620));
    let b = content_box(480, 800, 254, &r.with_status(0, 120)).unwrap();
    assert_eq!((b.y, b.height), (80, 600));
    let b = content_box(480, 800, 254, &r.with_status(60, 70)).unwrap();
    assert_eq!((b.y, b.height), (80, 640));
    let b = content_box(480, 800, 254, &r.with_status(100, 120)).unwrap();
    assert_eq!((b.y, b.height), (100, 580));
    // Status lines that leave no room for text: no page.
    assert_eq!(content_box(480, 800, 254, &r.with_status(400, 400)), None);
}

#[test]
fn page_actions_and_arrows_turn_the_page() {
    for a in ["core.next-page", "core.right", "core.down"] {
        assert_eq!(turn(&action(a)), Turn::Next);
    }
    for a in ["core.prev-page", "core.left", "core.up"] {
        assert_eq!(turn(&action(a)), Turn::Previous);
    }
    assert_eq!(turn(&action("core.confirm")), Turn::Ignored);
    assert_eq!(turn(&action("")), Turn::Ignored);
    let key = Input::Key {
        now_ms: 0,
        key: "page-forward",
        state: paperpaw_sdk::native::callbacks::KeyState::Press,
    };
    assert_eq!(turn(&key), Turn::Ignored);
}

#[test]
fn the_trail_goes_back_newest_first_and_is_bounded() {
    let mut trail = Trail::new();
    assert!(trail.is_empty() && trail.pop().is_none());
    trail.push((0, 0)).unwrap();
    trail.push((0, 40)).unwrap();
    assert_eq!(trail.pop(), Some((0, 40)));
    assert_eq!(trail.pop(), Some((0, 0)));
    for n in 0..(TRAIL as u32 + 10) {
        trail.push((0, n)).unwrap();
    }
    assert_eq!(trail.pop(), Some((0, TRAIL as u32 + 9)));
    let mut count = 1;
    while trail.pop().is_some() {
        count += 1;
    }
    assert_eq!(count, TRAIL);
    trail.push((1, 1)).unwrap();
    trail.clear();
    assert!(trail.is_empty());
}

/// Pages of 10 source units: the page after the one at `(s, o)` starts at `o + 10`.
fn tens(at: (u32, u32)) -> Result<(u32, u32), Error> {
    Ok((at.0, at.1 + 10))
}

#[test]
fn the_page_before_is_found_from_the_sections_start() {
    assert_eq!(
        page_before((0, 0), (0, 35), 100, tens).unwrap(),
        Some((0, 30))
    );
    assert_eq!(
        page_before((0, 0), (0, 30), 100, tens).unwrap(),
        Some((0, 20))
    );
    assert_eq!(
        page_before((0, 0), (0, 10), 100, tens).unwrap(),
        Some((0, 0))
    );
    // The first page has no page before.
    assert_eq!(page_before((0, 0), (0, 0), 100, tens).unwrap(), None);
    assert_eq!(page_before((2, 0), (1, 5), 100, tens).unwrap(), None);
}

#[test]
fn a_layout_that_does_not_move_on_or_runs_long_is_refused() {
    let stuck = |at: (u32, u32)| Ok(at);
    assert_eq!(page_before((0, 0), (0, 5), 100, stuck), Err(Error::NoSpace));
    assert_eq!(
        page_before((0, 0), (0, 5_000), 10, tens),
        Err(Error::NoSpace)
    );
    let refused = |_| Err(Error::NoMemory);
    assert_eq!(
        page_before((0, 0), (0, 5), 10, refused),
        Err(Error::NoMemory)
    );
}

#[test]
fn the_settings_that_change_the_layout_are_told_from_those_that_change_the_ink() {
    for key in [
        "core.font-size",
        "core.line-spacing",
        "core.word-spacing",
        "core.margin",
        "core.margin-ends",
        "core.justify",
        "core.reading-font",
        "core.hyphenate",
        "core.hyphen-min-left",
        "core.hyphen-min-right",
        "",
    ] {
        assert!(affects_layout(key), "{key}");
        assert!(!affects_ink(key), "{key}");
    }
    for key in [
        "core.smooth-text",
        "core.font-weight",
        "core.text-contrast",
        "core.text-sharpness",
        "core.text-curve",
    ] {
        assert!(affects_ink(key), "{key}");
        assert!(!affects_layout(key), "{key}");
    }
    // The publisher styles are applied when a book opens: no reflow applies them.
    for key in [
        "core.theme",
        "reader.hold-chapters",
        "core.brightness",
        "core.publisher-styles",
    ] {
        assert!(!affects_layout(key) && !affects_ink(key), "{key}");
    }
}
