//! What `tests/goal.rs` proves on interface 6 with a whole kernel. The decisions
//! that need no host (mood, goal, `today.txt`, when it is written) are tested in
//! `src/model/tests.rs`; these need host services that are not there yet and stay
//! ignored until they are. Each names what is missing.

#[test]
#[ignore = "needs the statistics to run the stage stats.read-today with the figure (stats side) and a host that admits stage subscriptions and runs native plugins end to end"]
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
#[ignore = "needs images (draft 22) and the widget host call (Core todo 76): the cat's four pictures, own folder setting"]
fn the_widget_shows_the_cat_of_todays_mood_and_the_minutes() {
    unreachable!(
        "widget part `minutes`, tap opens the stats goal setting, offers switch-on or store"
    );
}

#[test]
#[ignore = "needs images (draft 22) and the sleep plugin on interface 7 (choice sleep.show = reading-goal.sleep)"]
fn the_sleep_screen_shows_the_cat_large_in_the_middle() {
    unreachable!("the cat, the minutes and the goal below it");
}

#[test]
#[ignore = "needs core.device with the plugin list (is `stats` installed, enabled) and a restart command for switching it on"]
fn the_offer_switches_the_statistics_on_or_opens_the_store() {
    unreachable!("answer `switch-on` enables stats and restarts; `store` runs store.open");
}
