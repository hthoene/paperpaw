//! Decides how the panel is refreshed.

use paperpaw_platform::{Rect, RefreshMode};

/// After how many refreshes of the user interface (fast, or smaller than a
/// page) a full refresh cleans the panel; 0 never (as Beta 3).
pub(crate) const UI_FULL_REFRESH_INTERVAL: &str = "core.ui-full-refresh-interval";

/// How often the panel is cleaned: after `pages` page turns and after `ui`
/// refreshes of the user interface; 0 never.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Intervals {
    pub(crate) pages: u32,
    pub(crate) ui: u32,
}

/// Upgrades every n-th partial refresh of a page (most of the screen, as
/// a page turned) to a full refresh of the whole screen to remove
/// ghosting, as the setting says ("after this many page turns"): smaller
/// ones (a line of progress, a focus moved, the clock) neither count nor
/// flash. It counts the refreshes as they are done
/// ([`RefreshPolicy::done`]), after refresh filters changed them: a full
/// refresh a filter asked for starts the count anew.
///
/// The smaller ones and the fast ones (menus, a focus moved, notices: the
/// user interface) are counted apart, and where
/// [`UI_FULL_REFRESH_INTERVAL`] is set, the n-th of them is a full refresh
/// of the whole screen too: their ghosts would stay where no page is
/// turned (#255).
#[derive(Debug, Default)]
pub(crate) struct RefreshPolicy {
    partials: u32,
    /// Refreshes of the user interface (fast, or partial and smaller than
    /// a page) since the last full refresh.
    ui: u32,
    /// What the areas asked for since the last refresh add up to (they
    /// may overlap, and the region refreshed is the one rectangle around
    /// them): two items far apart are a region of most of the screen, but
    /// no page. `None`: the region is taken as it is.
    weight: Option<u32>,
    /// The first frame after the start is not counted (see
    /// [`RefreshPolicy::after_start`]).
    skip_first: bool,
}

impl RefreshPolicy {
    /// The policy of a kernel that has just started: the first frame shown
    /// is not counted as a page. The start shows its screen (the home) at
    /// once, and the plugin that opens the last book hears `event.started`
    /// on the first wake after it, so the book opens as the second frame
    /// where Beta 3 opened it in the start itself: counted, the fourth page
    /// turned would flash.
    pub(crate) fn after_start() -> Self {
        Self {
            skip_first: true,
            ..Self::default()
        }
    }

    /// Says what the areas drawn for the refresh that is decided and done
    /// next add up to (see the field), or `None` for what lies in the
    /// region.
    pub(crate) fn weigh(&mut self, weight: Option<u32>) {
        self.weight = weight;
    }

    /// Whether `region` is most of `screen` (half or more) as far as what
    /// was asked for in it goes: a page.
    fn is_page(&self, region: Rect, screen: Rect) -> bool {
        let area = |r: Rect| u32::from(r.width) * u32::from(r.height);
        let drawn = self.weight.map_or(area(region), |w| w.min(area(region)));
        drawn * 2 >= area(screen)
    }

    /// Returns the mode and region to refresh. An interval of 0 disables
    /// that periodic full refresh.
    pub(crate) fn decide(
        &self,
        requested: RefreshMode,
        dirty: Rect,
        screen: Rect,
        every: Intervals,
    ) -> (RefreshMode, Rect) {
        let due = |count: u32, interval: u32| interval > 0 && count.saturating_add(1) >= interval;
        match requested {
            RefreshMode::Partial | RefreshMode::Gray if self.is_page(dirty, screen) => {
                match due(self.partials, every.pages) {
                    true => (RefreshMode::Full, screen),
                    false => (requested, dirty),
                }
            }
            RefreshMode::Partial | RefreshMode::Gray | RefreshMode::Fast
                if due(self.ui, every.ui) =>
            {
                (RefreshMode::Full, screen)
            }
            mode => (mode, dirty),
        }
    }

    /// How many refreshes that leave ghosting were made since the last
    /// that cleaned the panel.
    pub(crate) fn since_full(&self) -> u32 {
        self.partials
    }

    /// A refresh in `mode` of `region` (of `screen`) was done. Refreshes of
    /// the user interface are counted only where `every.ui` is set: never
    /// while reading (a clock, a progress line), so that leaving the page
    /// does not flash at once.
    pub(crate) fn done(&mut self, mode: RefreshMode, region: Rect, screen: Rect, every: Intervals) {
        if core::mem::take(&mut self.skip_first)
            && !matches!(mode, RefreshMode::Full | RefreshMode::Clear)
        {
            return;
        }
        match mode {
            RefreshMode::Full | RefreshMode::Clear => (self.partials, self.ui) = (0, 0),
            RefreshMode::Partial | RefreshMode::Gray if self.is_page(region, screen) => {
                self.partials = self.partials.saturating_add(1)
            }
            // Smaller ones and fast ones: the user interface.
            RefreshMode::Partial | RefreshMode::Gray | RefreshMode::Fast if every.ui > 0 => {
                self.ui = self.ui.saturating_add(1)
            }
            // Black and white (games, pen strokes) leave ghosting as they
            // find it: such screens clean themselves.
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where the user interface counts (as in menus), at an interval too
    /// long to flash.
    const COUNTED: Intervals = Intervals { pages: 0, ui: 1000 };

    fn pages(pages: u32) -> Intervals {
        Intervals { pages, ui: 0 }
    }

    #[test]
    fn small_partials_never_flash_nor_count() {
        let mut policy = RefreshPolicy::default();
        let screen = Rect::new(0, 0, 480, 800);
        let line = Rect::new(0, 740, 480, 40);
        for _ in 0..50 {
            let (mode, region) = policy.decide(RefreshMode::Partial, line, screen, pages(6));
            assert_eq!((mode, region), (RefreshMode::Partial, line));
            policy.done(mode, region, screen, COUNTED);
        }
        assert_eq!(policy.since_full(), 0);
        // Pages still flash at the interval: the sixth.
        let page = Rect::new(0, 0, 480, 760);
        let modes: alloc::vec::Vec<RefreshMode> = (0..6)
            .map(|_| {
                let (mode, region) = policy.decide(RefreshMode::Partial, page, screen, pages(6));
                policy.done(mode, region, screen, COUNTED);
                mode
            })
            .collect();
        assert_eq!(modes[..5], [RefreshMode::Partial; 5]);
        assert_eq!(modes[5], RefreshMode::Full);
    }

    #[test]
    fn every_nth_partial_becomes_full() {
        let mut policy = RefreshPolicy::default();
        let dirty = Rect::new(0, 0, 10, 6);
        let screen = Rect::new(0, 0, 10, 10);
        let modes: [RefreshMode; 4] = core::array::from_fn(|_| {
            let (mode, _) = policy.decide(RefreshMode::Partial, dirty, screen, pages(3));
            policy.done(mode, screen, screen, COUNTED);
            mode
        });
        assert_eq!(
            modes,
            [
                RefreshMode::Partial,
                RefreshMode::Partial,
                RefreshMode::Full,
                RefreshMode::Partial
            ]
        );
    }

    #[test]
    fn fast_does_not_count_and_zero_disables() {
        let mut policy = RefreshPolicy::default();
        let r = Rect::new(0, 0, 1, 1);
        for _ in 0..10 {
            policy.done(RefreshMode::Fast, r, r, COUNTED);
            assert_eq!(
                policy.decide(RefreshMode::Fast, r, r, pages(2)).0,
                RefreshMode::Fast
            );
            policy.done(RefreshMode::Partial, r, r, COUNTED);
            assert_eq!(
                policy.decide(RefreshMode::Partial, r, r, pages(0)).0,
                RefreshMode::Partial
            );
        }
    }

    /// A full refresh done otherwise (asked for, or made so by a filter)
    /// starts the count anew; one that became partial does not.
    #[test]
    fn the_count_follows_the_refreshes_done() {
        let mut policy = RefreshPolicy::default();
        let r = Rect::new(0, 0, 1, 1);
        policy.done(RefreshMode::Partial, r, r, COUNTED);
        policy.done(RefreshMode::Full, r, r, COUNTED);
        assert_eq!(
            policy.decide(RefreshMode::Partial, r, r, pages(2)).0,
            RefreshMode::Partial
        );
        policy.done(RefreshMode::Partial, r, r, COUNTED);
        // The full refresh the policy wanted became partial: still due.
        policy.done(RefreshMode::Partial, r, r, COUNTED);
        assert_eq!(
            policy.decide(RefreshMode::Partial, r, r, pages(2)).0,
            RefreshMode::Full
        );
    }

    /// The frame the start shows is not a page turned; the next ones are.
    #[test]
    fn the_first_frame_after_the_start_is_not_counted() {
        let mut policy = RefreshPolicy::after_start();
        let screen = Rect::new(0, 0, 480, 800);
        policy.done(RefreshMode::Partial, screen, screen, COUNTED);
        assert_eq!(policy.partials, 0);
        policy.done(RefreshMode::Partial, screen, screen, COUNTED);
        assert_eq!(policy.partials, 1);
        // A full refresh as the first frame counts as what it is.
        let mut policy = RefreshPolicy::after_start();
        policy.done(RefreshMode::Full, screen, screen, COUNTED);
        policy.done(RefreshMode::Partial, screen, screen, COUNTED);
        assert_eq!(policy.partials, 1);
    }

    /// Focus moves, menus and notices (fast or small) count apart and
    /// flash the whole screen at the n-th, where an interval is set (#255);
    /// page turns keep their own count.
    #[test]
    fn the_user_interface_flashes_every_nth_where_set() {
        let mut policy = RefreshPolicy::default();
        let screen = Rect::new(0, 0, 480, 800);
        let item = Rect::new(0, 100, 480, 60);
        let every = Intervals { pages: 6, ui: 3 };
        let mut modes = alloc::vec::Vec::new();
        for requested in [
            RefreshMode::Fast,
            RefreshMode::Partial,
            RefreshMode::Fast,
            RefreshMode::Fast,
        ] {
            let (mode, region) = policy.decide(requested, item, screen, every);
            policy.done(mode, region, screen, every);
            modes.push((mode, region));
        }
        assert_eq!(
            modes,
            [
                (RefreshMode::Fast, item),
                (RefreshMode::Partial, item),
                (RefreshMode::Full, screen),
                (RefreshMode::Fast, item),
            ]
        );
        assert_eq!(policy.ui, 1);
        // A page turn neither counts there nor is flashed by that count.
        policy.done(RefreshMode::Fast, item, screen, every);
        let (mode, region) = policy.decide(RefreshMode::Partial, screen, screen, every);
        assert_eq!((mode, region), (RefreshMode::Partial, screen));
        policy.done(mode, region, screen, every);
        assert_eq!((policy.partials, policy.ui), (1, 2));
    }

    /// A full refresh starts both counts anew; black and white (games)
    /// and an interval of 0 never flash for the user interface.
    #[test]
    fn a_full_refresh_starts_both_counts_anew() {
        let mut policy = RefreshPolicy::default();
        let screen = Rect::new(0, 0, 480, 800);
        let item = Rect::new(0, 100, 480, 60);
        for _ in 0..5 {
            policy.done(RefreshMode::Fast, item, screen, COUNTED);
            policy.done(RefreshMode::Partial, screen, screen, COUNTED);
        }
        assert_eq!((policy.partials, policy.ui), (5, 5));
        let off = Intervals { pages: 0, ui: 0 };
        assert_eq!(
            policy.decide(RefreshMode::Fast, item, screen, off).0,
            RefreshMode::Fast
        );
        let ui = Intervals { pages: 0, ui: 2 };
        assert_eq!(
            policy.decide(RefreshMode::Mono, item, screen, ui).0,
            RefreshMode::Mono
        );
        policy.done(RefreshMode::Mono, item, screen, COUNTED);
        assert_eq!(policy.ui, 5);
        policy.done(RefreshMode::Clear, screen, screen, COUNTED);
        assert_eq!((policy.partials, policy.ui), (0, 0));
        assert_eq!(
            policy.decide(RefreshMode::Fast, item, screen, ui).0,
            RefreshMode::Fast
        );
    }

    /// While reading (the interval passed as 0) the user interface does
    /// not count: a clock or a progress line never adds up to a flash
    /// later in a menu.
    #[test]
    fn nothing_counts_where_the_interval_is_off() {
        let mut policy = RefreshPolicy::default();
        let screen = Rect::new(0, 0, 480, 800);
        let line = Rect::new(0, 740, 480, 40);
        let reading = Intervals { pages: 6, ui: 0 };
        for _ in 0..10 {
            let (mode, region) = policy.decide(RefreshMode::Partial, line, screen, reading);
            assert_eq!(mode, RefreshMode::Partial);
            policy.done(mode, region, screen, reading);
        }
        assert_eq!(policy.ui, 0);
        let menu = Intervals { pages: 6, ui: 3 };
        assert_eq!(
            policy.decide(RefreshMode::Fast, line, screen, menu).0,
            RefreshMode::Fast
        );
    }
}
