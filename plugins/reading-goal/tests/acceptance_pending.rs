//! What `tests/goal.rs` proves on interface 6 with a whole kernel. The decisions
//! that need no host (mood, goal, `today.txt`, when it is written) are tested in
//! `src/model/tests.rs`; these need host services that are not there yet and stay
//! ignored until they are. Each names what is missing.

#[test]
#[ignore = "needs a host that admits stages and runs native plugins end to end (the statistics' answer is plugins/stats tell_today)"]
fn the_statistics_tell_today_and_it_is_written_to_the_card_when_a_book_closes() {
    unreachable!(
        "read_today while a book is open is kept in memory; book_closed, suspend and stopping write today.txt"
    );
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
#[ignore = "blocked on PR #649 (the native sleep) and add_choice: the choice sleep.show = reading-goal.sleep and the sleep screen"]
fn the_sleep_screen_shows_the_cat_large_in_the_middle() {
    unreachable!("the cat, the minutes and the goal below it");
}
