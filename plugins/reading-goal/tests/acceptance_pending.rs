//! What `tests/goal.rs` proves on interface 6 with a whole kernel. The decisions
//! that need no host (mood, goal, `today.txt`, when it is written) are tested in
//! `src/model/tests.rs`; these need host services that are not there yet and stay
//! ignored until they are. Each names what is missing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "fixture failures fail the regression tests"
)]

#[cfg(feature = "linked")]
#[path = "../../test-support/hosted.rs"]
mod hosted;

#[test]
#[cfg(feature = "linked")]
fn the_statistics_tell_today_and_it_is_written_to_the_card_when_a_book_closes() {
    use paperpaw_sdk::{
        EventData, Instant, MemoryStorage, SystemStatus, WallClock, builtin::events,
    };
    let mut page = [0u8; 16];
    page[2..6].copy_from_slice(&1u32.to_le_bytes());
    page[6..10].copy_from_slice(&(20_000u32 * 86_400).to_le_bytes());
    page[10..12].copy_from_slice(&750u16.to_le_bytes());
    page[12..16].copy_from_slice(&10u32.to_le_bytes());
    let setup = hosted::Setup::x4().with_kernel(move |b| {
        b.storage(MemoryStorage::new().with(".paperpaw/plugins/stats/pages.bin", page))
    });
    let mut goal = hosted::install_in(
        &setup,
        paperpaw_plugin_reading_goal::NATIVE_ENTRY.entry(),
        "reading-goal",
        &["core.registry", "core.ui"],
    )
    .unwrap();
    hosted::also(
        &mut goal,
        paperpaw_plugin_stats::NATIVE_ENTRY.entry(),
        "stats",
        &[
            "core.library",
            "core.net",
            "core.registry",
            "core.secrets",
            "core.ui",
        ],
        paperpaw_core::native_host::FixedTables::NONE,
    )
    .unwrap();
    let _ = goal.kernel().start(Instant(0));
    let _ = goal.kernel().set_status(
        Instant(0),
        SystemStatus::new(
            Some(WallClock::new(Instant(0), 20_000 * 86_400_000, Some(0))),
            None,
        ),
    );
    let reading = events::Reading::new("book.epub".into(), "Book".into(), 0.0, 0, 0, "".into());
    let _ = goal.kernel().send_event(
        Instant(0),
        events::BOOK_OPENED.id().clone(),
        reading.to_params(),
    );
    let _ = goal.kernel().send_event(
        Instant(0),
        events::BOOK_CLOSED.id().clone(),
        reading.to_params(),
    );
    assert!(goal.dropped().is_empty(), "{:?}", goal.dropped());
    let mut bytes = [0; 64];
    let n = goal
        .kernel()
        .storage()
        .read_at(".paperpaw/plugins/reading-goal/today.txt", 0, &mut bytes)
        .unwrap_or_else(|error| {
            panic!(
                "stage did not write: {error:?}; warnings={:?}",
                goal.kernel().take_warnings()
            )
        });
    assert_eq!(&bytes[..n], b"20000 750\n");
    assert!(goal.dropped().is_empty(), "{:?}", goal.dropped());
}

#[test]
#[ignore = "needs a host that runs native plugins end to end: today.txt is written through the data folder (the store is tested over MemoryStorage)"]
fn today_txt_is_read_and_replaced_only_when_it_differs() {
    unreachable!("parse_told on the file, told_text into a 32-byte buffer, one write per change");
}

#[test]
#[ignore = "needs a host that runs native plugins end to end with images: the pictures asked for are prepared and drawn (the decisions are tested in src/native_picture/tests.rs; the installed cats need PrepareRoot::Bundle, Services todo 308, Host todo 309)"]
fn the_widget_shows_the_cat_of_todays_mood_and_the_minutes() {
    unreachable!("the picture of today's mood, as large as its place leaves, above the minutes");
}

#[test]
#[ignore = "needs a host that composes widgets (H1, H3) and screen commands: the widget drawn in its frame, a tap opening the settings on the goal"]
fn the_widget_is_drawn_and_a_tap_opens_the_goal() {
    unreachable!(
        "frame, mood in words, `12 / 20 min`; a tap where the statistics count opens stats.goal"
    );
}

#[test]
#[ignore = "needs a host that composes the slot core.sleep with the native sleep (PR #649) and runs native plugins end to end: choosing sleep.show = reading-goal.sleep opens this screen, the cat is drawn large in the middle"]
fn the_sleep_screen_shows_the_cat_large_in_the_middle() {
    unreachable!("the cat, the minutes and the goal below it");
}
