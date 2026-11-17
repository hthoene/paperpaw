//! Status items: small pieces of information shown in a status line, such
//! as the time, the battery or the page.
//!
//! Any plugin adds items with
//! `Items::add_status_item`;
//! screens with a status line let the user choose which items it shows and
//! where (the reader does this for its lines above and below the text).
//! An item computes its value from the environment and from the context the
//! screen passes, e.g. the reader's `title`, `chapter`, `page` and
//! `progress` (see `context`).
//!
//! An item may do something when it is tapped ([`StatusItemSpec::tap`]:
//! the battery opens its settings), and says how much it matters
//! ([`StatusItemSpec::priority`]): where a line has no room for all its
//! items, those that matter least are left out first, never cut.
//!
//! Plugins can also add whole status lines
//! (`Items::add_status_line`),
//! which draw the line themselves: a progress bar, a large clock, anything.
//! The reader offers the lines the user chose, above and below the text,
//! and switches between them when the line is tapped.
//!
//! The places where status items show are [`Place`]s
//! (`Items::add_status_place`):
//! three settings of the items for a line's left, centre and right. The
//! reader has one above and one below the text, the home screen one in
//! its status widget; a status line drawn in a place is told which
//! ([`StatusFrame::place`]).

/// Parameters screens pass to status items. Items ignore what they do not
/// need; a screen passes what it knows.
pub(crate) mod context {}

/// The items of a list as stored in a setting: ids separated by commas.
pub fn item_ids(list: &str) -> impl Iterator<Item = &str> {
    list.split(',').map(str::trim).filter(|id| !id.is_empty())
}
