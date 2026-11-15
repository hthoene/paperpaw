//! What a page was laid out for, looked at again when it is shown after another
//! screen was over it.
use super::{Dashboard, Host, Snapshot, What, grid, native_pages, snapshot};

/// What a page was laid out for: the display (width, height), whether it has a
/// recent shelf, and whether that had a book (the shelf is no rows without one).
#[derive(Debug)]
pub(super) struct Kept {
    display: (u16, u16),
    shelf: bool,
    recent: bool,
}

impl Kept {
    pub(super) fn of(host: &Host<'_>, display: &Snapshot, shown: &[grid::Item]) -> Self {
        let shelf = shown.iter().any(
            |item| matches!(&item.what, What::Widget(id) if native_pages::base(id) == "home.recent"),
        );
        Self {
            display: (display.display_width(), display.display_height()),
            shelf,
            recent: shelf && has_recent(host) == Some(true),
        }
    }

    /// Whether the display or the shelf's books are other than they were.
    pub(super) fn differs(&self, host: &Host<'_>, display: &Snapshot) -> bool {
        (display.display_width(), display.display_height()) != self.display
            || (self.shelf && has_recent(host) != Some(self.recent))
    }
}

/// Whether the recent shelf has a book (the card shows the first): `None` where
/// the host refuses.
fn has_recent(host: &Host<'_>) -> Option<bool> {
    crate::native_recent::read_listed(host, 1, 1)
        .ok()
        .map(|books| !books.is_empty())
}

impl Dashboard {
    /// A setting or the registry changed (or a plugin came or went) while
    /// another screen is over this one: the page it kept is laid out again when
    /// it is shown. Where it is on top and was shown again a moment ago (the
    /// event reached it after the show's draw), it answers `true`: to be drawn
    /// again.
    pub fn invalidate_covered(&mut self) -> bool {
        if self.covered || self.returned {
            self.stale = true;
            false
        } else if self.fresh {
            self.stale = true;
            self.fresh = false;
            true
        } else {
            false
        }
    }

    /// Whether what a page kept under another screen cannot show any more: the
    /// display changed, or the recent shelf came or went (the books it shows are
    /// read when it draws, the shelf is no rows without one).
    pub(super) fn changed_while_covered(&self, host: &Host<'_>) -> bool {
        let Some(laid) = &self.laid else {
            return true;
        };
        let Ok(display) = snapshot::capture_snapshot(host) else {
            return true;
        };
        laid.kept.differs(host, &display)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown_and_drawn(covered: bool) -> Dashboard {
        let mut dashboard = Dashboard::new();
        dashboard.stale = false;
        dashboard.covered = covered;
        dashboard.shown(None);
        // The draw that follows the show takes the mark.
        dashboard.returned = false;
        dashboard.stale = false;
        dashboard
    }

    #[test]
    fn a_change_that_reaches_the_page_after_its_show_lays_it_out_again() {
        let mut dashboard = shown_and_drawn(true);
        assert!(dashboard.invalidate_covered());
        assert!(dashboard.stale);
        // Once: the next change is for the draw that follows.
        dashboard.stale = false;
        assert!(!dashboard.invalidate_covered());
        assert!(!dashboard.stale);
    }

    #[test]
    fn the_minute_ends_the_window_of_a_page_just_shown() {
        let mut dashboard = shown_and_drawn(true);
        dashboard.mark_stale();
        dashboard.stale = false;
        assert!(!dashboard.invalidate_covered());
        assert!(!dashboard.stale);
    }

    #[test]
    fn a_change_under_another_screen_marks_the_page_and_draws_nothing() {
        let mut dashboard = Dashboard::new();
        dashboard.stale = false;
        dashboard.covered = true;
        assert!(!dashboard.invalidate_covered());
        assert!(dashboard.stale);
    }

    #[test]
    fn a_show_nothing_said_was_covered_lays_the_page_out_again() {
        let mut dashboard = Dashboard::new();
        dashboard.stale = false;
        dashboard.shown(None);
        assert!(dashboard.stale);
        assert!(!shown_and_drawn(true).stale);
    }
}
