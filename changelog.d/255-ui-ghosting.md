### Added

- **Full refresh in menus** (#255, lab). A new advanced Display setting, `core.ui-full-refresh-interval`, counts the refreshes of the user interface (fast ones and those smaller than a page: focus moves, menus, settings, notices) apart from page turns and flashes the whole screen at the n-th, so ghosting no longer builds up where no page is turned. 0 (the default) keeps Beta 3's behaviour; page turns keep their own count, and a full refresh starts both counts anew. While the reading screen is on top, nothing counts there: a clock or progress line never leads to an extra flash.
