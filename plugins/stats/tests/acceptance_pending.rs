//! What the interface-6 integration tests (`recording.rs`, `screens.rs`, `widgets.rs`,
//! `sync.rs`, `books.rs`) prove with a whole kernel and the plugin in it. The data layer
//! is tested in `src/store.rs`, `src/store/hostile_tests.rs` and `src/files.rs`; these need
//! host services or ported parts that are not there yet and stay ignored. Each names the
//! missing piece.

#[test]
#[ignore = "needs a host that runs native plugins end to end: it refuses icon declarations today, so make fails (the recording itself is tested in src/recorder/tests.rs)"]
fn pages_read_are_recorded_and_the_days_summed() {
    unreachable!("page turned/book closed end a stay; append to pages.bin; days() on a new day");
}

#[test]
#[ignore = "needs core.ui screens on interface 7 (overview, calendar, books, book) and the stats.flow fixture of #321"]
fn the_screens_show_time_streaks_calendar_and_books() {
    unreachable!("overview 'Today', calendar months, book list, one book's pages read");
}

#[test]
#[ignore = "needs the widget and status stages (host call, Core todo 76)"]
fn the_widgets_and_the_status_item_show_todays_reading() {
    unreachable!("today, week, ring, streak, calendar, year widgets; left.rs status item");
}

#[test]
#[ignore = "needs a host that admits stage subscriptions, and a native reading-goal that runs reading-goal.read-today (PR #450)"]
fn reading_goal_is_told_todays_seconds() {
    unreachable!(
        "the stage answers day and seconds from memory (Recorder::asked), ERR_NOT_FOUND for a day it does not know"
    );
}

#[test]
#[ignore = "needs core.net (#351), secrets (Services todo 79) and core.files listing (#339)"]
fn webdav_sync_merges_and_uploads_with_an_etag() {
    unreachable!("download, merge_as, conditional upload, working-file sweep");
}

#[test]
#[ignore = "needs a way for a plugin to set another plugin's default (interface 6: set_setting_default); stats declares 20 minutes itself"]
fn a_goal_is_none_until_reading_goal_is_installed() {
    unreachable!(
        "stats.goal defaults to none alone and to 20 minutes where reading-goal is installed"
    );
}
