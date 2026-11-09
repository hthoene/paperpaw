//! Reading goal: the daily goal of the reading statistics (`stats.goal`,
//! 20 minutes at first where this plugin is), and a cat whose mood follows
//! how much of it was read today:
//! sad while nothing was, meh after a little, content from half the goal
//! (`reading-goal.content`, in percent), happy once it is reached
//! (`reading-goal.happy`).
//!
//! - *Reading cat* (`reading-goal.widget`), a widget for the home screen
//!   and the sleep screen's widgets: the cat of today's mood, as large as
//!   its place leaves, and below it the minutes read and the goal (part
//!   `minutes`). A tap opens the goal's setting; where the statistics are
//!   off it offers to switch them on, where they are missing to get them
//!   from the store.
//!
//! The pictures are PaperPaw's cats (see `NOTICE`), or one's own: the
//! folder `reading-goal.pictures` names holds `sad`, `meh`, `content` and
//! `happy` (`.png`, `.jpg`), any size; those missing are the cats'.
//!
//! Today's reading comes from the reading statistics (the `stats`
//! plugin), which answer the stage `reading-goal.read-today`; it is kept in
//! `today.txt` (`<day> <seconds>`, version 1), while a book is open in
//! memory only, written when it closes or the device sleeps or stops: not
//! at every page turned. Without them, nothing is counted, and the widget
//! says so. Without a goal (set to none), the cat goes by [`model::DEFAULT_GOAL`]
//! minutes and only the minutes read show.
//!
//! - A sleep screen of its own (*Today's reading cat* among the sleep
//!   screen's choices): the cat large in the middle, the minutes and the
//!   goal below it.
//!
//! This is the interface-7 plugin only (`PORTING-7.md`).

#![no_std]

extern crate alloc;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
mod declarations;
mod native_entry;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
/// The interface-7 plugin.
pub mod native_plugin;

pub mod model;
mod native_picture;
mod native_sleep;
mod native_widget;
pub mod store;

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
