//! The arrow keys stay on the page on a fresh device (#465): from the top
//! of the home page, up goes round to its bottom. A reader who turned it
//! off keeps it off, and up stays at the top.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use crate::common::*;
use paperpaw_sdk::builtin::settings::ARROWS_STAY_ON_PAGE;

#[test]
fn on_a_fresh_device_the_arrow_keys_stay_on_the_page() {
    for (config, stays) in [("", true), ("arrows-stay-on-page = off\n", false)] {
        let mut app = App::new(X4, Shared::memory(config, &[]));
        assert_eq!(
            app.kernel.settings().read(&ARROWS_STAY_ON_PAGE),
            stays,
            "{config:?}"
        );
        // The first press shows the focus where it is, on the top row.
        app.press("up");
        let top = app.focused();
        assert!(top.is_some(), "{}", app.described());
        app.press("up");
        assert_eq!(
            app.focused() != top,
            stays,
            "{config:?}: {}",
            app.described()
        );
    }
}
