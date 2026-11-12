### Fixed

- **A page under an overlay can redraw** (#243). `command.redraw` and `command.redraw_area` of a
  native screen that has only overlays above it (the reader's page under its menu bar, a picture
  finishing) were refused or dropped, and a redraw hit the top screen's region. They now act on
  that screen's own region, with the overlays drawn again over it (a dimming one too); a full
  screen or a guard above it still gives `ERR_STATE`, and a queued redraw is dropped then.
