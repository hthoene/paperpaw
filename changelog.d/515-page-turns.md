### Fixed

- **Books after the first are counted at the device's speed again** (#515, #518).
  On the X4 Pro and X4 Classic every book opened after the first one counted
  its pages at the small X4's pace, a page a second for many minutes, and a
  page turn often waited for one of these steps. The count now steps at the
  device's pace again. Background work of any plugin no longer runs between
  pressing a button and the action it makes, and work that comes due during a
  button's own handling runs right after its frame is drawn, so the turned page
  shows first and the count goes on after it. Measured on the X4 Pro, the
  release of a page turn while a book was counted took 1,074 to 2,000 ms before
  and 21 to 340 ms now.
