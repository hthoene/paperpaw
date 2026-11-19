# Simple home (#236): a native interface-7 store plugin

Version 1.0.0. Concept D of the launcher mock-ups (`senior/236-launcher-mocks`), no covers (Decision 256).
Not built into the firmware: a store plugin in the publish set (x4, x4-pro, x4-classic), linked in the
emulator and shipped off like the other store plugins. Nothing is published.

- Screens: `home` (offered for `core.home`; hides the host's status bar, draws its own date and battery
  line), `panel` (offered for `core.panel`), `library`, `customise` (opens the menu editor on one of the
  three menus below).
- Customising is three menus the user edits in Settings > Edit menus: `launcher-simple.cards` (the card of
  the book being read is its action `launcher-simple.continue`, other rows are slim cards), `.dock` (up to
  five icons, any action) and `.panel` (a switch setting is a round switch, a number setting a bar, an
  action a round button; a row that needs a front light is not listed where there is none). The panel is
  also shown for any other menu it is opened with.
- What it holds: the rows and the books are read once and kept; the registry, a setting, the library or the
  screen shown again make the next frame read them anew. A key press only moves the focus (`model::step`,
  geometric with a wrap at the edges) and draws the two parts it touches (`queue_redraw_area`); the minute's
  tick draws the status line and the clock only. Touch: a tap uses the part, a tap on a bar sets the value.
- Room: a card the home has no room for (at the largest text) is left out from the end; the dock holds
  five icons, the panel eight switches and three bars, the library lists 60 books.
- Icons are the kit's monochrome icon font; the clock's digits are drawn from round strokes (fonts end near
  48 px).
- Memory: three small row lists, up to 60 book rows (title, author), one paint list per frame.
  Not measured on a device.

Tests: `src/model.rs` (focus, taps, bars, paging), the linked entry, the screens' snapshots by name.
