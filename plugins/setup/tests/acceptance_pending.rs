//! What the interface-6 tests (`guide.rs`, `guide_walk.rs`, `switch_step.rs`,
//! `wander.rs`, `card.rs`) proved with a whole kernel and the plugin in it,
//! drawing real pixels. The decisions of the pages are tested without a host
//! (`src/model/tests.rs`); these need the host to run a native plugin end to
//! end, and stay ignored until it does. Each names what is missing.

#[test]
#[ignore = "needs a host that runs native screens and replays their draw ops (Core H1), and registry op 15 with steps declared by plugins"]
fn the_guide_walks_every_step_and_ends_on_where_books_come_from() {
    unreachable!("pixel golden of setup.guide on 480x800 Gray4, en and de, each page");
}

#[test]
#[ignore = "needs the host to admit callback subscriptions of a plugin and the library service (LIB) for the first-start check"]
fn the_first_start_opens_the_guide_once_and_never_on_a_used_device() {
    unreachable!("Started: no done file, no book opened: guide in core.setup; done file written when left");
}

#[test]
#[ignore = "needs the registry's setting reads (op 2, op 3) and set_setting commands dispatched to a native plugin"]
fn a_number_a_switch_and_a_choice_change_their_setting() {
    unreachable!("text size − and +, preview while the buttons change it; switch; two options; list");
}

#[test]
#[ignore = "needs menu entries and icons admitted by the host (records menu.entry and icon.spec are refused today)"]
fn set_up_paperpaw_is_in_the_settings_before_about() {
    unreachable!("core.settings-pages entry setup.open at order 97 with the wand icon");
}

#[test]
#[ignore = "needs core.ui hand-over (H2) to open the settings plugin's list with parameters; choices are asked with the host dialog meanwhile"]
fn a_choice_of_many_opens_the_settings_list() {
    unreachable!("core.choose-value with id and focus for the language and the time zone");
}
