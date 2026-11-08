use super::*;

#[test]
fn moods_follow_the_share_of_the_goal() {
    let of = |minutes: u32| Mood::of(minutes * 60, 20, 50, 100);
    assert_eq!(Mood::of(59, 20, 50, 100), Mood::Sad);
    assert_eq!(of(0), Mood::Sad);
    assert_eq!(of(1), Mood::Meh);
    assert_eq!(of(9), Mood::Meh);
    assert_eq!(of(10), Mood::Content);
    assert_eq!(of(19), Mood::Content);
    assert_eq!(of(20), Mood::Happy);
    assert_eq!(of(500), Mood::Happy);
    // Thresholds as set.
    assert_eq!(Mood::of(15 * 60, 20, 80, 150), Mood::Meh);
    assert_eq!(Mood::of(29 * 60, 20, 80, 150), Mood::Content);
    assert_eq!(Mood::of(30 * 60, 20, 80, 150), Mood::Happy);
    assert_eq!(Mood::of(u32::MAX, 0, 50, 100), Mood::Happy);
    assert_eq!(Mood::of(u32::MAX, u32::MAX, 50, u32::MAX), Mood::Meh);
}

#[test]
fn the_moods_are_named_for_their_pictures() {
    let names: [&str; 4] = Mood::ALL.map(Mood::name);
    assert_eq!(names, ["sad", "meh", "content", "happy"]);
}

#[test]
fn the_goal_is_the_minutes_of_the_statistics_setting() {
    assert_eq!(goal_minutes("stats.goal.30"), Some(30));
    assert_eq!(goal_minutes("stats.goal.off"), None);
    assert_eq!(goal_minutes("stats.goal.0"), None);
    assert_eq!(goal_minutes("stats.goal.-5"), None);
    assert_eq!(goal_minutes("stats.goal.99999999999"), None);
    assert_eq!(goal_minutes(""), None);
    assert_eq!(goal_minutes("45"), Some(45));
}

#[test]
fn today_txt_is_a_day_and_seconds_and_nothing_else() {
    assert_eq!(parse_told(b"20400 1234\n"), Some((20_400, 1234)));
    assert_eq!(parse_told(b"-3 0"), Some((-3, 0)));
    for bad in [
        &b""[..],
        b"20400",
        b"20400 ",
        b"a b",
        b"20400 -1",
        b"20400 1 2",
        b"20400 4294967296",
        b"99999999999 1",
        b"\xff\xfe 1",
    ] {
        assert_eq!(parse_told(bad), None, "{bad:?}");
    }
}

#[test]
fn today_txt_round_trips_and_a_small_buffer_is_refused() {
    for (day, seconds) in [
        (0, 0),
        (20_400, 1234),
        (-1, 7),
        (i32::MIN, u32::MAX),
        (i32::MAX, u32::MAX),
    ] {
        let mut out = [0u8; 32];
        let text = told_text(day, seconds, &mut out).unwrap();
        assert!(text.ends_with('\n'));
        assert_eq!(parse_told(text.as_bytes()), Some((day, seconds)));
    }
    let mut out = [0u8; 32];
    assert_eq!(told_text(20_400, 1234, &mut out), Some("20400 1234\n"));
    for len in 0..11 {
        assert_eq!(told_text(20_400, 1234, &mut out[..len]), None, "{len}");
    }
}

#[test]
fn only_todays_seconds_count() {
    let on = |told, today| Today::of("stats.goal.20", told, today, 50, 100, Statistics::Counting);
    let day = Some(20_400);
    assert_eq!(on(Some((20_400, 600)), day).minutes, 10);
    assert_eq!(on(Some((20_400, 600)), day).mood, Mood::Content);
    // Yesterday's, or a device that does not know the day: nothing read.
    assert_eq!(on(Some((20_399, 600)), day).mood, Mood::Sad);
    assert_eq!(on(Some((20_400, 600)), None).minutes, 0);
    assert_eq!(on(None, day).mood, Mood::Sad);
}

#[test]
fn without_a_goal_the_cat_goes_by_twenty_minutes() {
    let today = Today::of(
        "stats.goal.off",
        Some((1, 20 * 60)),
        Some(1),
        50,
        100,
        Statistics::Counting,
    );
    assert_eq!(today.goal, None);
    assert_eq!(today.mood, Mood::Happy);
    assert_eq!(today.minutes, 20);
}

#[test]
fn thresholds_are_clamped_as_the_settings_are() {
    let mood = |content, happy| {
        Today::of(
            "stats.goal.10",
            Some((1, 300)),
            Some(1),
            content,
            happy,
            Statistics::Counting,
        )
        .mood
    };
    // 300 s of 600 s = 50 %.
    assert_eq!(mood(50, 100), Mood::Content);
    assert_eq!(mood(0, 100), Mood::Content);
    assert_eq!(mood(i64::MAX, 100), Mood::Meh);
    assert_eq!(mood(50, i64::MIN), Mood::Happy);
    assert_eq!(mood(i64::MIN, i64::MAX), Mood::Content);
}

#[test]
fn the_statistics_are_counting_off_or_missing() {
    assert_eq!(Statistics::of(Some(true)), Statistics::Counting);
    assert_eq!(Statistics::of(Some(false)), Statistics::Off);
    assert_eq!(Statistics::of(None), Statistics::Missing);
}

#[test]
fn what_was_told_is_written_when_no_book_is_open_or_it_closes_or_sleeps() {
    let mut kept = Kept::new();
    assert_eq!(kept.told(Some((1, 5))), Some((1, 5)));
    assert_eq!(kept.to_write(Some((1, 5))), None);
    // No book open: written at once.
    assert!(kept.read_today(2, 60));
    assert_eq!(kept.told(Some((1, 5))), Some((2, 60)));
    assert_eq!(kept.to_write(Some((1, 5))), Some((2, 60)));
    assert_eq!(kept.to_write(Some((2, 60))), None);
    // A book open: not at every page turned.
    kept.book_opened();
    assert!(!kept.read_today(2, 120));
    assert!(!kept.read_today(2, 180));
    assert!(kept.book_closed());
    assert_eq!(kept.to_write(Some((2, 60))), Some((2, 180)));
    assert!(kept.must_flush());
    assert!(kept.read_today(2, 240));
}

#[test]
fn the_stage_payload_is_exactly_a_day_and_the_seconds() {
    let mut bytes = [0u8; 8];
    bytes[..4].copy_from_slice(&20_000i32.to_le_bytes());
    bytes[4..].copy_from_slice(&750u32.to_le_bytes());
    assert_eq!(parse_stage(&bytes), Some((20_000, 750)));
    bytes[..4].copy_from_slice(&(-1i32).to_le_bytes());
    bytes[4..].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(parse_stage(&bytes), Some((-1, u32::MAX)));
    for len in [0, 1, 4, 7, 9, 16] {
        assert_eq!(parse_stage(&[1u8; 16][..len]), None, "{len}");
    }
}
