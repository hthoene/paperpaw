### Fixed

- **The row of button hints goes away cleanly** (#483). Where a window
  that has no hints opened over a page while the row showed after a press,
  the old labels stayed on the screen for buttons that now did something
  else. A press while the row shows no longer redraws it each time (over a
  page in grays that was the whole screen): it only stays a little longer.
