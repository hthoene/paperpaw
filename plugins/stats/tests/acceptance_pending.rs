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
#[ignore = "every widget and the status items are native (tests/scripted_widgets.rs, widgets_parity.rs, src/native_widget_year/tests.rs, src/native_left/tests.rs); what is left is their pixels (a kernel-backed harness), the year widget's lookup against a real core.library and the status stage's reply, which the scripted host cannot give an event"]
fn the_widgets_and_the_status_item_show_todays_reading() {
    unreachable!(
        "pixels of the widgets; books this year with the library; the time-left status text"
    );
}

#[test]
#[ignore = "needs a host that admits stage declarations, and a native reading-goal that asks stats.read-today"]
fn reading_goal_is_told_todays_seconds() {
    unreachable!("the stage answers day and seconds as the stay is recorded");
}

#[test]
#[ignore = "needs a host that runs native plugins end to end: the engine (src/native_webdav_tests.rs) and the shell are tested without it, the requests need core.net with a WebDAV server"]
fn webdav_sync_merges_and_uploads_with_an_etag() {
    unreachable!("download, merge_as, conditional upload, working-file sweep");
}
