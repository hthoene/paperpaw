# Changelog

What changes from one release of PaperPaw to the next. Versions follow
[Semantic Versioning](https://semver.org); before 1.0, a minor version may
change the plugin SDK.

## [Unreleased]

### Added

- *End of book*, a new plugin: when the last page of a book is reached, a screen offers the next book of its
  series (or the library), a rating of one to five (`4 / 5`, kept as a book field and shown as a badge on the
  book's row and on its information page) and whether the book counts as finished. Two settings in the reading
  group: *Show a screen at the end of a book* and *Mark the book as finished* (both on). It needs no SDK change.

### Changed

- How an X4, X3, X4 Pro or X4 Classic starts, runs the trial of and
  installs an update moved from the firmware into `paperpaw-x4`
  (`updates`), which the firmware runs as before: CI now tests the whole
  update path on a computer, the update plugin on the kernel installing
  online onto a flash with the tables PaperPaw ships, with the release
  before's real update file and the firmware built in the run, then once
  more to a higher version; and a damaged update, a download cut short,
  one too large and a trial that fails, none of which may take the
  firmware (`crates/paperpaw-x4/tests/update_path.rs`, CI job
  `update-path`).
- X4 and X3: the power button (and *Sleep after*) now put the reader to a real sleep: the screen stays, the processor
  sleeps lightly and a press of the power button wakes it at once where it was (no start). Only after a while asleep
  on battery does it switch itself off completely, as it used to do at once. A new setting, *Switch off after
  sleeping* (Power: at once, 15 or 30 minutes (the default), 1 or 2 hours, never), says when; *Never* keeps it asleep
  (it uses a little battery all the time, and still switches off when the battery is nearly empty). While it
  charges it stays asleep. *Sleep lightly on battery* (developer group) off, or the choice *At once*, is the old way.
  The X4 Pro and X4 Classic sleep as before. Not yet tested on a device.

### Fixed

- A book read to its last page is now recorded as read to 100 % (it was recorded as how far the start of its last page
  is, often 95 %, so the library did not list it as finished). The reader's `core.book-finished` and
  `core.book-closed` now carry 100 once the last page was reached.
- A time zone rule with an offset of more than 24 hours (such as `<>200`) is now refused instead of read as an offset of days.

## [0.5.0-beta.12] - 2026-10-28

### Changed

- X4 (with an SSD1677 panel, as most have): a setting, *Fast changes* (Display), says how menus and a moved focus
  are shown: the panel's own waveform (500 ms, the cleanest) or a quicker
  one of 8, 6 or 4 frames (found in the waveform lab; about 160 ms with 4),
  which leave more ghosting that the next full refresh clears. It is
  *The panel's own* (the slowest, the cleanest) by default; page turns are as before.
  Black-and-white refreshes (screens that ask for `fastest`, such as games)
  take about 100 ms, with 1 frame. The frames and the kick of both are
  settings of the developer group (`x4.ssd1677.fast-frames`, `x4.ssd1677.fast-kick`,
  `x4.ssd1677.mono-frames`, `x4.ssd1677.mono-kick`, offered only where the panel's
  controller is an SSD1677; the fast ones where *Fast changes* is
  *Own values*), so a plugin without code can set them in its `settings.ini`
  or by an action; the firmware keeps them in the order of the modes (fast
  at most 20 frames, black and white no more than fast).

### Added

- X4 Pro (a UC8179 controller): the fastest refresh (screens that ask for `fastest`,
  such as games) has numbers of its own, the developer settings `x4.uc8179.mono-frames`
  (12), `x4.uc8179.mono-hold` (0) and `x4.uc8179.mono-rate` (200 Hz): about 175 ms
  by the formula (3 frames at 100 Hz were far too grey, 6 nearly clean: the drive
  needs about 60 ms, whatever the rate). The frame rates go up to 200 Hz (110, 130,
  150, 200 added); the log says how long the setup and the refresh waited for the
  controller. It took the step of *Fast changes* (7 frames, hold 3,
  about 330 ms, no ghosts) before. The numbers are a guess, to tune on a device.
- X4 Pro (a UC8179 controller): a developer setting, `x4.uc8179.transfer`, says how much
  of the picture goes to the controller for a fast change: the whole (what the
  stock firmware sends), only the rows and columns that changed (into both
  memories), or only into the new memory, leaving the controller's own copy to
  make the old one (the default: about 50 ms less a fast change, 329 instead of
  379 ms at 7 frames, judged right on a device). The refresh itself
  still covers the whole display, so the hold keeps working. The whole picture
  goes again after a start, a sleep, grays and the slow refreshes.

- X4 Pro (a UC8179 controller): *Fast changes* (Display) says how menus and a moved
  focus are shown, as on the X4: the panel's own waveform (625 ms) or a quicker
  one of the controller's registers, run at the voltages of the panel's own
  waveform (read from its OTP at start, about 65 ms more): 6, 7 or 10 (the panel's own is the default)
  frames with 3 held (about 360, 380 and 440 ms), or own values in the developer
  settings (`x4.uc8179.*`). Found with the waveform lab on a device.
- The waveform lab's direct drive on the X4 Pro can hold the pixels that stay (0 to 3
  frames driven to their own colour, as the controller's own fast waveform does
  for one frame), which keeps the ghosts of earlier frames down.
- The waveform lab's direct drive on the X4 Pro runs at the voltages and VCOM of the
  controller's own waveform (read from its OTP at start), not the reset values
  of its registers (VDH and VDL 14.0 V, VCOM_DC next to nothing); and the lab can
  show the panel's own waveform at a temperature (up to 120 degrees) on a UC8179
  too.
- Developer firmware of the X4 Pro (`x4pro` builds) reads the controller's OTP at start (a
  UC8179, about 100 ms) and tells the log what its own waveforms are: the
  temperature ranges, their frame rate, voltages and VCOM, and the states of the
  ranges 25 and 90 degrees select (`otp:` lines).
- The waveform lab also runs on the X4 Pro (a UC8179 controller): a direct drive
  made of the controller's waveform registers, with the frames, the kick, the
  frame rate (20 to 100 Hz), the repeat and the levels of white and black to try,
  at the controller's own voltages. Developer firmware (`x4pro` builds).
- `tools/flash-x4.py` builds the checked-out source and flashes it to an X4,
  X4 Pro, X4 Classic or X3 over USB, with a check of the tools, an optional
  backup of the reader's flash and a dry run.
- The developer firmware of the X4 has a *Waveform lab* under Tools, for
  trying how the panel is driven: its own tables at a temperature, or a
  direct drive with the frames and the kick chosen on the screen.

### Changed

- X4 (with an SSD1677 panel, as most have): a setting, *Fast changes* (Display), says how menus and a moved focus
  are shown: the panel's own waveform (500 ms, the cleanest) or a quicker
  one of 8, 6 or 4 frames (found in the waveform lab; about 160 ms with 4),
  which leave more ghosting that the next full refresh clears. It is
  *Quicker, some ghosting* (6 frames) by default; page turns are as before.
  Black-and-white refreshes (screens that ask for `fastest`, such as games)
  take about 100 ms, with 1 frame. The frames and the kick of both are
  settings of the developer group (`x4.ssd1677.fast-frames`, `x4.ssd1677.fast-kick`,
  `x4.ssd1677.mono-frames`, `x4.ssd1677.mono-kick`, offered only where the panel's
  controller is an SSD1677; the fast ones where *Fast changes* is
  *Own values*), so a plugin without code can set them in its `settings.ini`
  or by an action; the firmware keeps them in the order of the modes (fast
  at most 20 frames, black and white no more than fast).
- EPUB chapters convert and pages read faster, books open with fewer file
  lookups, and skimming to the next or previous chapter no longer converts
  every chapter of the book first.
- Folders on the SD card are read faster, and reading a large file at
  scattered places no longer walks its clusters from the start each time.
- Page turns finish sooner: the X4 Pro and X4 Classic no longer send each
  frame a second time after its refresh, and released firmware no longer
  checks every frame it sends.
- Text, light separator lines and shaded highlights draw faster in black and
  white and in grays, with the same pixels as before.
- Menus, settings pages and the home screen spend less time on their texts
  and settings, and a start reads the translations faster: a message is
  looked for only in the catalogs that can hold it.
- Large libraries open faster: the library's shelves and its index read
  the catalog once, a shelf shown again after a book or its actions is not
  read anew when nothing changed, home keeps its covers made small, and
  the vocabulary list reads only the words of the page shown.
- Book pages draw faster: lines outside the part of the screen being drawn
  are skipped instead of measured word by word. Searching a book needs less
  memory.
- Turning a page waits less for the card: the reading position and
  today's reading are written once the pages rest for a moment, the book
  closes or the device sleeps, and the book's details are read only when
  its shown percentage changes.
- Pages are laid out faster: hyphenation stops looking for patterns that
  cannot match, and characters are found in font packs with fewer lookups.
  Lines break exactly as before.
- Lists, menus and settings pages draw faster on readers that draw the
  screen in bands, such as the X4: rows, headers and status lines outside
  the band being drawn are skipped instead of laid out again for each band.
- A page turns as soon as its button goes down: holding a page button
  for the next or previous chapter is now off by default and can be
  switched on in the settings under Controls ("Hold for next chapter",
  an advanced setting).
- *Button response* on the X4s has a third choice, *Saving*: the processor
  sleeps up to 50 ms between readings of the buttons on battery (30 ms
  as *Normal*, 10 ms as *Faster*), for a little less current; a tap
  shorter than that may be missed. *Normal* stays as it was.
- Rules: the list to choose an action, a switch or a number from comes by
  plugin where it is longer than a page (some six pages of actions on the
  X4): first the plugins, with how many each brings, then its own, and
  back goes out of a plugin again. Every action stays on offer, also those
  that work on the screen shown.

### Fixed

- X4 and X4 Pro: a screen that runs at a pace (`ScreenMode::frame_every`) showed its
  frames with the waveform the last frame before it had left on the panel, not that of
  its own refresh mode. A game with `ScreenMode::fastest` and `grays(2)` kept the panel's
  own waveform (500 ms) until a frame that did not wait for the panel (a game-over
  card, say) set the quick one, and so seemed to be fast only after the first restart.
  Paced frames now set the waveform and the transfer as every other frame does.
- X4 Pro: the sleep screen with a cover, a picture or the cat (shown in grays) is made
  after a refresh of every pixel, as the others are (a page with pictures is not: it
  keeps the quicker first stage, also at its periodic full refresh); before, the grays were put over a fast
  refresh, which leaves a little ghosting of the screen shown before it. The fast
  refresh before any grays also no longer takes a quicker waveform that a game or
  a menu left set.
- Moving the focus between two items far apart on the home screen (the edit pencil
  and a *Continue reading* widget with a cover) no longer refreshes the whole panel
  in full at every step. The kernel took the rectangle round the two items for a page
  (half the screen or more) and, with a large picture on the screen, refreshed it in
  full; it also counted every such step towards the full refresh every few pages. It
  now goes by what was drawn (the areas added up), not by the rectangle round them;
  a whole screen drawn anew, a page, is as before. A widget of half the screen or
  more (the *Continue reading* widget with its cover) is a page by itself, and
  every focus step still showed the screen in four grays (about 1.7 s): a focus step
  in the widget grid now sets its refresh exactly (fast), as the kernel takes it for
  a step and not for a page. The first step after a screen was shown in grays still
  drives every pixel once.
- The row of hints that comes on a press (in the reader, with the buttons below the
  screen) is shown and taken away with a normal partial refresh, not a fast one: with
  the quicker waveforms of *Fast changes* it left ghosts of the page's text at the
  bottom at every page turned by those buttons.
- On the X4 and X4 Pro, what plugins do on waking now happens: *When the
  device wakes up* in the rules, *Review on waking* of the flash cards, and
  syncing a position at wake. Their sleep restarts the device, which plugins
  heard as a start only; the firmware leaves a mark (`.paperpaw/slept`) as
  it sleeps, and the start that finds it sends `core.resume` after
  `core.started`. The battery log keeps one start line for the two.
- The flash cards' *Retention wanted* setting shows its value as a
  percentage again, not as the key `vocabulary.unit.percent`: the unit
  needs its plural form (`.other = {count} %`).
- The side margins set for reading no longer get the book's own `body`
  margin and padding added on top, which left the sides wider than the
  setting (top and bottom were right). Books are converted again.

## [0.5.0-beta.11] - 2026-10-26

### Added

- X4 Pro firmware includes Bluetooth LE for HID page-turners and
  keyboards, pairing on the reader, and plugin GATT clients. Remotes
  remain manual by default; active Bluetooth prevents processor light
  sleep, and device sleep shuts the radio down. Other readers are
  unchanged. Advertising and plugin GATT servers remain unavailable.

### Fixed

- Bluetooth applies the requested remote connection parameters before HID
  discovery, so setup no longer blocks that change. Idle power policy is unchanged.
- Bluetooth reconnects known devices when the controller reports their resolved
  identity or a new private address, while retaining the existing paired entry.

## [0.5.0-beta.10] - 2026-10-26

### Fixed

- Justified text keeps bounded word spacing when font size or word
  spacing changes, without abruptly switching whole lines to tight
  spaces. Balanced line breaking considers the gaps it can distribute.
  The settings preview and book pages use the same behavior.
- X4 plugin-cache indexes are read with the current entry size, so a
  restart reuses verified cached code instead of rewriting every plugin.
- Xteink readers enter hardware sleep even when USB or charging is
  detected, instead of keeping the processor awake behind the sleep
  screen. Press the physical power button to wake them.
- A sleep request survives redrawing the screen after a grayscale image.
- X4 light-sleep transitions cannot overlap the button ADC interrupt's
  access to the internal analog bus.
- The X4 Pro holds battery power before memory, diagnostics and OTA
  startup work, so a short power press need not span that work.
- Screens that hide button hints, including the sleep screen, cannot
  retain a temporary hint row from the previous screen.

### Changed

- Xteink readers show a startup message before loading plugins on normal
  starts too. Its panel refresh overlaps plugin loading.
- X4 Pro and X4 Classic verify native plugin digests with the chip's
  SHA-256 engine. The same header, image and relocation bytes are checked
  before any plugin code runs.
- Kernel startup reads installed native-plugin digests once for the plugins
  it will load, after recovering pending installs. The bounded snapshot is
  discarded after loading; the plugin interface is unchanged.

### Added

- Text layout callers can add a cost for the unused width and internal
  spaces of a candidate balanced line with
  `wrap_lines_balanced_kerned_with_cost`. Existing calls keep their layout.


### Compatibility

- The native plugin interface remains i4, with the same pinned compilers.
  Existing compatible plugins continue to run. Reader layout caches are
  rebuilt for the updated justification behavior.

## [0.5.0-beta.9] - 2026-10-26

### Fixed

- Every restart shows a translated restart notice before saving state
  and resetting the reader, whichever menu requested it.
- The USB development helper reads current format 3 plugin digests when
  trusting a local build, while still accepting format 2 files.
- Plugins selected in the web flasher keep their Store origin after
  installation, so the plugin store can update them automatically.
  Manually installed plugins keep their local origin.
- First startup shows plugin checking, installation progress and startup
  preparation while the web flasher's bundle is copied to the SD card.
- Settings distinguish *Manage plugins* from *Plugin settings*, rather
  than showing two entries named *Plugins*.

- Resetting settings changes only explicitly stored values, avoiding
  repeated font and layout reloads for defaults that were never changed.
  Book-specific resets leave global values intact.

- The plugin store shows when it refreshes a saved plugin list, while
  keeping that list available until the request finishes.

- Crates.io releases use new versions for changed built-in plugins, so packaged applications compile against the matching plugin sources.

- A text shortened to its room (`ui::fit`, rows' titles and values)
  counts kerning as it is drawn: pairs set apart (`f\` in the UI font)
  could take it up to a few pixels past its room, into the value beside
  it.

## [0.5.0-beta.8] - 2026-10-25

### Changed

- Wi-Fi joins the saved network picked last on the Wi-Fi page first,
  when it is in reach, instead of the strongest: after a restart the
  reader stays in the network one chose. *Join first* (Last picked,
  Strongest) sets it.
- **Code plugins must be built again for beta.8.** The plugin interface
  is now 4: what crosses changed for a book's formatting and kerning
  (`Block`, `TextStyle`, `Typesetter`, `layout::Style`, `FontHandle`),
  the settings' pages (`SettingGroupSpec`), screens that run in real
  time, screens opened once, static registration (`Actions::get`,
  `Env::menus`, `Env::screens`) and lists that draw only what a step
  changes (`ListChange`), with room for what comes later (the developer
  link, `Command::Extended`), and room between the lines for page
  layers (`Block::leading_above`, `PageLine::leading`,
  `LineBox::leading`, `PageView::words`, `PageLayerSpec::words`),
  blocks filters insert (`Block::inserted`, `PageLine::inserted`),
  input on layers (`PageLayerSpec::items`, `LayerItem`,
  `PageLayerTapped::item`) and room for annotations
  (`Block::annotations`, `PageLine::annotations`).
  PaperPaw's own store plugins come as updates.
- Moving through the settings with the buttons is quicker: a step draws
  only the row left and the row reached, no longer the whole screen (on
  the X4 a sixth to a third of it), in every list of the settings.
- The firmware is leaner, so updates keep room: German hyphenation is a
  plugin of the store (`hyphenation-de`), as the other languages are,
  and the built-in one keeps English; Literata Bold Italic is no longer
  built into the firmware (bold stands in for it; the emulator and the
  app keep it), and its faces go up to 32 px there (about 10 pt). A data
  plugin may bring faces of a family without its regular one, and a pack
  that extends a face with larger sizes draws text that large (the font
  size goes as far), so the font library brings both back
  (`tools/literata-packs.py`). At the largest size, its help says where
  larger ones are found.
- The release keeps room for the updates to come: `paperpaw-sign
  check-image` prints the margin an image leaves in the smallest app
  slot (its own or CrossPoint's), warns below 512 KB and fails below
  256 KB.
- A step with the buttons draws only what it changes, on every screen
  with a list, a grid or buttons to move between: the item the focus
  left and the one it came to, no longer the whole screen (on the X4 a
  sixth to a third of a frame). `ListChange::Focus` carries the two
  items, so `ListChange::request` asks for them alone; `ListView`,
  `Rows` and `Sections` give `focus_change` for a focus that only came
  to show, `ListChange::focus` serves focuses of a screen's own and
  `ui::button_reach` says where a focused button draws. Their `request`
  methods are gone: `change.request(cx)` does it. A screen whose focus
  changes more than its items asks for that part too.
- The store finds its way in a library of thousands: its start shows
  what the library picks (Recommended, New, Popular now, Recently
  updated, Discover) between the search and the groups, and a group
  lists its plugins in the library's fair mix of the day, so that every
  plugin gets seen. A to Z stays a choice, sorted on the device a run at
  a time, and long groups go to a letter (*Go to letter*, a grid of A to
  Z for buttons and fingers). The list still streams to the card. A
  library without picks shows the start and the order as before.
- Plugins that work with another product say what they do with it:
  KOReader highlights, Calibre connection, Readwise export, Wallabag
  articles, Wikipedia lookup and Joplin notes (their ids stay, and so do their
  settings and data).
- Literata's sizes above 32 px and its Bold Italic, which the X4's
  firmware leaves out, are plugins of the store: the release builds
  PaperPaw's own data plugins (`tools/plugin-files/data.toml`) and
  publishes them with the others, versions checked as theirs.
- With buttons alone (the X4) the keyboard keeps a computer keyboard's
  rows at every size (qwertyuiop above asdfghjkl); the size makes keys
  higher and their letters larger, and a row is split only where its
  letters would not fit. Up and down keep to the column one came along.
  Typing takes about as many presses as before (627 against 633 for the
  sample texts in `plugins/keyboard/tests/presses.rs`), capitals fewer.
- Its keys are wider with buttons alone: the focus fills a key instead
  of framing it, so two pixels between keys do (on the X4 a letter's key
  is 39 pixels wide, was 25). On touch screens the room between keys
  follows the screen's size (0.6 mm). The plain X4s start with the
  keyboard at Small, the X4 Pro at Large; a size chosen stays. "Hold: ö"
  in place of "Hold for ö" leaves the field's title whole.
- Settings where readers look for them: *Date and time* (the clock, the
  first day of the week, the time zone), *Plugins* (each plugin's
  network) and *Words* are groups of their own; the auto sleep is under
  *Power*, the device's name under *Wi-Fi*, the sleep screen's logo with
  the sleep screen, the widgets' frame with the home screen. Reading's
  settings have shorter names (*Invert pictures*, *Paragraph spacing*,
  *Top and bottom margins* …).

### Added

- Plugins write between a book's lines, an interlinear reading with a
  translation above each line: a text filter asks for room above every
  line of a block (`Block::leading_above`, hundredths of an em, at most
  four), which the layout adds to the line's height so pages break with
  it, and each line tells it (`PageLine::leading`, `LineBox::leading`);
  a page layer that asks (`PageLayerSpec::words`) gets the page's words
  with their boxes and places (`PageView::words`, `reading::WordBox`,
  now without the feature `unstable`), measured once a page and only
  where a layer asks.
- Text filters insert blocks after the one they get
  (`Block::insert_after`: a translation after each paragraph, a gloss,
  a note), at most four with 4 KiB of text: shown after it, never apart
  from its last line, with no place in the book (`PageLine::inserted`),
  so positions, progress, bookmarks, highlights, search and the page's
  words stay the book's.
- Page layers take input on what they draw: a layer says where its items
  are (`PageLayerSpec::items`, `LayerItem`); a tap on one, or Select on
  one chosen with the buttons as links are, comes to its plugin
  (`PAGE_LAYER_TAPPED` with `item`), and every other tap and button
  stays the reader's. The room above a line counts as the line's
  (`PageView::line_at`).
- Room for annotations over words (ruby, furigana, phonetics, glosses):
  `Block::annotations` and `PageLine::annotations`, empty until the
  layout sets them (ADR 0032).
- A sample plugin, `samples/interlinear`: a translation above each line
  that opens on a tap, or after each paragraph.
- With buttons alone, holding Back finishes typing and keeps the text,
  wherever the focus is (Back alone cancels); the field says so. Holding
  Select still finishes on any key that is no letter.
- The keyboard locks capitals with *Shift* pressed twice in a row (the
  key filled until pressed again), and a long press on a letter offers
  its capital first, chosen: with buttons a long press and Select type
  it. *Automatic capitals* (off by default) starts a text and each
  sentence with one, not in passwords, numbers and addresses.
- SDK: `ask::TextQuestion::url()` and `email()` ask for a web or e-mail
  address (modes `url` and `email`), typed as it is. The browser,
  catalogs, news, KOReader sync, Joplin, Wallabag and the statistics'
  sync ask for their addresses so.
- Plugins can run in real time: games, emulators, animations
  ([ADR 0030](docs/adr/0030-real-time-screens.md)). A screen says how
  it runs in one place (`Screen::mode`, `realtime::ScreenMode`): full
  screen, buttons pressed and released (`Screen::on_key`,
  `Env::keys_held`; holding *Back*, or a finger in the top left corner,
  two seconds always leaves), its own refreshes, the fastest refresh,
  the grays enough, a pace, staying awake longer, and the memory it
  needs. The device says what it can do and how the last frame went
  (`Env::panel`, `PanelInfo`, `FrameReport`), so faster panels make
  plugins faster without a new build. `Canvas::blit` draws packed
  bitmaps at a whole scale, `memory::vec_in` keeps data in internal RAM
  or PSRAM, and code plugins get `Api::alloc_in`. On the X4 the main
  loop goes on while the panel refreshes for such a screen, and every
  frame's times are measured. `samples/frame-test` shows it all. The
  first time such a screen opens, a notice says how to leave it.
- Room for a screen that needs memory: the kernel lets caches go, holds
  background work back, switches Wi-Fi and Bluetooth off where the
  screen does not need them, asks the screens below to let go of what
  they make again (the reader its book, keeping its place;
  `Screen::on_squeeze`, `RoomSteps::SQUEEZED`) and closes them where
  that is not enough, until there is enough; everything comes back once
  it closes (`ScreenMode::memory`, `Env::room`).
- A screen opened again while it is open is gone back to, not stacked
  (several *Transfer over Wi-Fi* screens one over another took as many
  presses of *Back*): the screens above it close and it hears the
  params (`Screen::on_reopen`). Each screen says how many of it may be
  open (`ScreenSpec::instances`, `Instances`): one for each set of
  params (the default), one at most (the transfer, Wi-Fi, Bluetooth,
  store and diagnostics screens and other roots) or many (the
  keyboard). Params are alike in any order.
- Plugins see what the buttons are good for: `Capability::PAGE_KEYS`
  and `Capability::BUTTONS` (enough for every screen without touch),
  from the buttons the shell lists.
- Less memory at rest: actions, screens, menu entries and icons can be
  `static` lists that stay in flash (`FixedAction` and
  `Registrar::add_actions`, `FixedScreen` and `add_screens`,
  `add_menu_entries`, `add_icons`; `MenuEntry::new` is `const`), as the
  kernel's and the plugins' are now. `Actions::get` gives an `Action`
  of either kind; `Env::screens` gives a `ScreenList`. Menus no longer copy the labels of the actions and
  settings they list (`Env::menus` gives a `MenuView`). Translations
  keep only the languages in use as catalogs; English catalogs a
  language covers drop their index; the indexes of the catalogs built
  in are made when PaperPaw is built (`FixedIndexes`,
  `KernelBuilder::catalog_indexes`). The X4 emulator's home at start
  takes some 30 KiB less.
- Room for a screen that needs memory may also draw frames in more,
  smaller bands (`RoomSteps::BANDS`), as they were once it closes.
- Plugins can keep the device awake for a while (`power::WakeHold`,
  `Command::KeepAwake`, `Cx::keep_awake`): bounded, renewed by id, a
  screen's ended with it, never with a battery nearly empty. *Let
  plugins keep the device awake* (on) turns it off for all; the usage
  says each plugin's awake time.
- Hot code in code plugins (`paperpaw_sdk::hot!`): the X4 runs it from
  internal RAM, a bounded arena whose rest goes to the heap. Plugin
  files are of format 3 (the hot range and its patches); RISC-V plugins
  link with `--no-relax`.
- Room in the plugin interface for what comes later without a new one:
  `Command::Extended`, `Screen::on_extended`, `Contribution::Extended`,
  `Capabilities::has` with `Capability` constants, wider `Moments` and
  `BookStyles`, `Storage::write_at` and `truncate`.
- The reading settings in pages of their own: *Text*, *Layout*,
  *Hyphenation*, *Publisher's formatting*, *Display*, *Status lines*,
  *Marks in the text*, *Reading aloud*, *Reading aids* and *Behaviour*,
  each short, its rarely needed settings one level down under *More*
  (or with the others, with *Show advanced settings*). Settings about
  reading came along: the colours of text and paper (from *Look*),
  night mode, text darkness, pictures in grays and full refresh (from
  *Display*), the voice (from *System*) and *Battery low at* (now with
  the status lines). SDK: groups within groups
  (`SettingGroupSpec::within`, `Settings::parent_of` and `within`),
  `groups::READING_TEXT` and the other parts of `READING`;
  `STATUS_LINES` and `MARKS` are parts of it.
- Balanced line breaking: paragraphs are broken over all their lines,
  so lines end more evenly and justified text has fewer wide gaps;
  *Line breaking* can be set back to *Fast*, line by line
  (`core.line-breaking`). SDK: `text::wrap_lines_balanced`,
  `layout::Style::balanced`.
- A book's own styles one by one: with *Publisher's formatting* on, its
  alignment, indents, text sizes, spacing, fonts, margins and line
  spacing can each be left to the reader (its line spacing is by
  default). SDK: `layout::BookStyles`, `Style::book`, settings
  `core.book-*`.
- More of a book's formatting comes along: margins at the sides (block
  quotes, poems, lists set in), hanging indents, line heights,
  superscript and subscript, underline and strike, numbered and nested
  lists, and a table's rows each on a line of their own. SDK:
  `Block::margins`, `Block::line_height`, `TextStyle::SUPERSCRIPT`,
  `SUBSCRIPT`, `UNDERLINE` and `STRIKE` (a `TextStyle` has 16 bits now).
- Letter spacing, and the size of indents and of the space between
  paragraphs, as settings (`core.letter-spacing`, `core.indent-size`,
  `core.paragraph-space`); their defaults keep pages as they were.
- Kerning and ligatures: pairs such as *AV* and *To* are set as the
  font kerns them (its GPOS `kern` feature, else its legacy `kern`
  table), in books and menus, and *fi* and *fl* (*ff*, *ffi* and *ffl*
  where they take the room of their letters) are drawn as ligatures in
  books where the font has them. Settings *Kerning* and *Ligatures*
  (advanced, per book, on: `core.kerning`, `core.ligatures`); books are
  laid out again once. Font packs keep a kerning table of classes, read
  in place (still version 1: older firmware ignores it, older packs read
  without kerning); the default packs grow by about 134 KB of flash, no
  memory. SDK: `Font::kern`, `BitmapFont::kern`, `OutlineFont::kern` and
  `kerning_pairs`, `PackBuilder::kerning`, `charset::LIGATURES`,
  `FontHandle::kern`, `unkerned` and `ligatures`,
  `layout::Typesetter::kern`, `text::wrap_lines_kerned` and
  `wrap_lines_balanced_kerned`, `PageType::kerning` and `ligatures`,
  `settings::KERNING` and `LIGATURES`; `text::measure`, `draw_text` and
  `FontHandle::measure` and its drawing kern the text they get.
- Hyphenation for more languages, as store plugins of their own: French,
  Spanish, Italian, Dutch, Portuguese, Swedish, Danish, Norwegian
  (Bokmål), Polish, Russian and Ukrainian. The firmware keeps
  German and English; each language takes flash, almost no memory.
- Settings *Letters before hyphen* and *Letters after hyphen*
  (advanced, 0 to 5; 0: the language's own): how short a piece of a word
  may be. Books are laid out again when they change. SDK:
  `settings::HYPHEN_MIN_LEFT`, `HYPHEN_MIN_RIGHT`,
  `PageType::hyphen_minimums` and `patterns`,
  `hyphen::Patterns::with_minimums`.
- *Text contrast* (Settings, Reading): book text lighter, normal, darker
  or darkest, its letters' edges on a contrast curve, finer than *Font
  weight* and together with it. Normal draws as before; each book may
  have its own.
- *Smooth text* works on every screen now: off draws book text in black
  and white, without gray edges, also where the screen has grays (the
  Boox), on at first there. On the X4s it stays off at first, and off,
  pages of text turn without the slower refresh in grays. It is a
  setting of the kernel now (`core.smooth-text`, was
  `reader.smooth-text`, taken over), each book may have its own. SDK:
  `settings::SMOOTH_TEXT`, `settings::TEXT_CONTRAST`, `text::ToneCurve`,
  `FontHandle::draw_toned`, `Canvas::mono_coverage`.
- Room for USB developer tools as a plugin ([ADR 0031](docs/adr/0031-developer-link.md)):
  `Uses::DEVELOPER`, `Capability::USB_LINK`, `Command::Developer` with
  `developer::open`, `write`, `screen` and `close`, heard as `DevEvent`.
  Reserved: this release answers that the link is not available.
- SDK: `Ui::filled_button` and `filled_icon_button`, filled as an `on`
  button without saying it is on (a key whose fill is the focus).

### Fixed

- The X4s no longer crash when a server does not answer at all: the
  radio starts anew for such a connection, and the clock's question to
  the time server, kept from before, then reached into the new network
  stack. Handles of what the stack made now carry its number, and a
  later stack passes over an earlier one's (also in beta.7).
- A download to a file, or an upload from one, no longer breaks off
  after a time in all (10 minutes for downloads, 30 s for uploads from a
  file): it fails only once no byte came for a minute, so a large update
  on a slow network goes through. SDK: `HttpOptions::timeout` means that
  for them.
- The network picked last (*Join first*) is looked for again when a scan
  misses it, and gets a second try when joining it fails, before another
  network joined before is joined: the first scans after the radio came
  on often miss a weaker network, and one of its access points, weak or
  far, refuses a join now and then.
- The X4s leave the Wi-Fi network before they start again (after an
  update, a restart from the settings) and before they sleep. A router
  that was not told (a FRITZ!Box) refused their joins for a while after,
  so they joined another network or none.

## [0.5.0-beta.7] - 2026-10-25

### Added

- The notices of everything PaperPaw ships: the crates of the firmware
  and of the apps, each under its licence with its copyright lines and
  each licence's text once, and Mbed TLS, ESP-IDF's radio libraries and
  bootloader, the FreeInk and OpenX4 display code and Papyrix. Made from
  the lockfiles by `tools/licences/build.py` into NOTICE and About
  PaperPaw, Licences (packed, written out to read part by part); CI
  fails where a crate that ships is missing. The web, emulator,
  handbook and core library archives carry LICENSE and NOTICE.
- Weather names Open-Meteo, Wikipedia names each article below its
  summary, with their licences.
- The emulator starts online with `--wifi online` (over `--stdio`,
  `"wifi": "online"` in `start`): the X4s on USB, their radio on and in
- The emulator starts online with `--wifi online` (also given before
  `--stdio`, for the devices it starts; or `"wifi": "online"` in
  `start`): the X4s on USB, their radio on and in
  a network `Workshop Wi-Fi` joined before, so plugins' requests go out
  at once; Wi-Fi switches off and on in the settings as on a device.
  Without it, nothing changes.
- Plugins can be published from GitHub: tag a release in the plugin's
  own repository (topic `paperpaw-plugin`) and paperpaw.org lists it as
  "Community, unreviewed". `tools/dev/templates` has the release
  workflow that zips the folder, builds and signs a code plugin and
  makes the release; the handbook and the plugin skill explain it.
- `paperpaw-dev check-publish` (and `paperpaw-sign check-publish`)
  checks a plugin's repository and its release files by the plugin
  store's rules, the site's own (`tools/publish/store-rules.json`, served
  by paperpaw.org too), and says what to change; the release workflow
  runs it before it builds and on the files it made, and releases
  nothing where it fails. `paperpaw-dev new --github <id>` makes a
  repository that passes it, the workflow included.
- Community plugins: the store's new setting *Community plugins*
  (Settings, System, off at first) also lists plugins that their authors
  published on GitHub, which nobody reviewed. They are marked
  *Unreviewed*, with the repository they come from; a code plugin's page
  shows the key it is signed with.

### Changed

- Settings sorted where readers look for them (their ids kept, so what
  was set stays): pages of their own for the Keyboard (layouts, umlaut
  keys, size, shape of keys), the Screen lock (with Lock touch), Updates,
  the Sleep screen (from Display) and the Home screen (from Appearance);
  Record battery use with Power, Show hidden files with the Library,
  Number of test pages with Developer, the reading cat's settings with
  Reading statistics; Calibre and News in parts of their own in the
  Library. Hidden where they do nothing: the shape of keys on readers
  with buttons alone, the choice of gray covers on gray screens. Clearer
  names: Lines at the top and at the bottom (the status lines), Covers in
  the library and in catalogs, Selected row, Line thickness, List
  sections, the vocabulary's two switches; percentages with their unit.
- Keyboard size works on readers with buttons alone too: Large splits
  the rows into short ones (few presses along each, as before and the
  X4s' default), Medium splits the longest, Small keeps them whole as on
  touch screens (a lower keyboard, more presses).
- The on-screen keyboard: on readers with buttons alone (the X4s, the X3)
  the focus starts on *Phone*, where the text can be typed on a phone
  instead of a letter at a time; on the X4 Pro the keys are large by
  default (a size chosen before stays).
- On the Xteink readers the interface font, drawn from bitmap packs made
  from Source Sans 3, is PaperPaw Sans: its licence reserves the name
  for the unchanged font. A reading or status font chosen as Source Sans
  3 is kept. The unchanged font keeps its name where it is drawn as it
  is.
- A code plugin's key is trusted from its first install: an update or a
  new install after a removal signed by another key is refused with a
  short warning, from any library; there is no *Install anyway* any
  more.
- Firmware is installed only with the battery at 20 % or more, or on
  USB: below it the kernel refuses any install (the update, another
  firmware), checked as the writing would begin, and *Update* says
  "Charge to 20 % or plug in to install." Downloading goes on. SDK:
  `InstallFailure::LowBattery`, `firmware::INSTALL_BATTERY`,
  `firmware::battery_allows_install`.
- No surprise restart after an update: while the new firmware runs on
  trial, without its code plugins, the X4 shows "Finishing the
  update…" with a bar that fills, takes no input meanwhile, and starts
  again from there once the update is kept, with its code plugins. SDK:
  `builtin::screens::FINISHING_UPDATE`.
- Devices are counted without being known: the update search says
  whether the reader asked (`trigger`: `auto` or `manual`) and of which
  periods it is the device's first search (`first`: `day`, `week`,
  `month`, in UTC; left out while the clock is not set). No id and no
  request of its own; the device keeps only the date of its last search.
- Back to CrossPoint without a computer: *Update* installs another
  firmware's image from the card (not signed, only as chosen, with a
  warning), checked whole for this device first. SDK:
  `FirmwareCommand::InstallForeign`, `firmware::ForeignImage` and
  `firmware::ForeignCheck`.
- Bluetooth Low Energy for plugins (ADR 0027): `paperpaw_sdk::bluetooth`
  scans, connects and reads, writes and subscribes to a device's
  characteristics, pairs (always confirmed by the reader in the kernel's
  own question), advertises and serves GATT services, for plugins that
  declare `Uses::BLUETOOTH`. The kernel owns the radio, shares it
  between plugins, lets each see only the devices it found or was given,
  keeps bonds with the passwords and keys and switches the radio off
  when nothing needs it. Cheap by default: scans and advertising end
  after a while, passive and seldom unless asked, links at long
  intervals, the radio at the cheapest power that serves every plugin,
  and its time on counted (`Usage::bluetooth_ms`). A bonded remote's or keyboard's keys are
  buttons of the key maps (`button.hid.right`, `button.hid.page-down` …,
  pages turned by default). The emulator has made-up devices for the X4
  Pro's profile: a page-turner (`ble-key:right` in scripts) and a
  heart-rate belt.
- Bluetooth page (Settings, where there is Bluetooth): the devices bonded
  with the reader, Add a device (a search, then pairing confirmed on the
  reader), forgetting one (the menu button or a long press), which
  plugins may use a sensor, and Remotes: when a remote's or a keyboard's
  keys work (when switched on, while reading, or always). A remote that
  falls asleep connects again by itself when it wakes, as long as its
  keys are wanted.
- **Code plugins must be built again.** The plugin interface is now 3
  (`Command`, `Env`, `Capabilities`, `MenuNeed`, `Usage`, `Work` and
  `FirmwareCommand` changed, for Bluetooth and another firmware): a code
  plugin built for an earlier release does not run on this one until it
  is built again with this release's SDK, and the plugin manager says
  so. PaperPaw's own store plugins come as updates. `Work`'s `network`
  and `charging` became `needs` (`Needs::NETWORK | Needs::CHARGING`).

### Fixed

- Uploads over Wi-Fi no longer stall when the card fails to take a part:
  the X4s answer at once, while the file still comes (*500* where the
  card failed, *507* where it is full), instead of holding the upload
  until it timed out. The X4 Pro tries a write to a card still busy
  writing more often before it gives up, as some cards are while the
  radio sends.
- Status lines stand every item on one line: the battery, icons and bars
  centred on the figures of the time, the page and the percentage, and
  those in the middle of the line, to the pixel. The battery stood a
  pixel higher than its percentage.
- Customise menus names the menus of the font choosers (their way to
  more fonts) as the settings are named, e.g. *Status lines › Font*,
  instead of showing `core.status-font` and `core.reading-font`. A test
  walks the menu editor, every settings page and chooser and every
  menu in each language shipped and fails on an id shown as a name.
- Test builds for the X4 Pro and the X4 Classic (with remote control
  over USB) took no updates over the air ("OTA: no partitions for
  updates"): their image's two data segments hid which half of the
  flash runs. The firmware now finds it either way. Released firmware
  was not affected. The release now fails for an image that could not
  take the next update (`paperpaw-sign check-image`: two app slots in
  its table, one data segment, room in either slot and CrossPoint's),
  and host tests run whole updates with every partition table shipped
  and CrossPoint's.

## [0.5.0-beta.6] - 2026-10-25

### Added

- The web flasher can put every store plugin on a reader at once: it
  writes the plugins chosen into the reader's empty second app
  partition, where all of them fit (about 4.8 MB on the X4), instead of
  the room behind the firmware (about 1.9 MB). PaperPaw installs them
  onto the card at its first start and only then frees the partition
  for updates; a power cut in between installs them at the next start.
  The room behind the firmware is still read, for older flashers.

## [0.5.0-beta.5] - 2026-10-25

### Fixed

- After the Wi-Fi link is lost and joined again, the X4s count as
  online only once the router gave their address again. Before, they
  were online at once with the old one, which the router's answer then
  took back, so a request made in between failed.
- Touch lock: the way out it explains (tap twice, hold the second tap)
  unlocks; it wanted a third touch held after two taps, which still
  works too.
- Choosing another Wi-Fi network while joined to one works on the X4s:
  they leave the first network before joining, instead of failing with
  "did not answer". Networks are joined with the security the scan found
  (WPA and mixed WPA/WPA2 ones too), and a password that was not taken
  for another reason than being wrong is offered again, not typed anew.
- News: articles with a long body in a CDATA section (WordPress feeds, The
  Verge) show their whole text; the reader dropped any section over 4 KiB
  and fell back to the summary. The XML reader now passes a long CDATA
  section on in pieces, in the same small memory. Dates, Atom
  and RSS, show short as the language writes them (Sat 24 Oct 2026).

### Added

- The web flasher can write almost the whole store with the firmware:
  the X4s take its bundle of plugins from the other app partition too,
  empty on a first flash (8 MB), not only from behind the image (about
  2 MB), and say so in the image for the flasher to find.

- Switching from CrossPoint without a computer: each release has an
  install file per device (`paperpaw-<device>-<version>-install.bin`)
  for CrossPoint's *SD firmware update*, and CI fails a build that no
  longer fits CrossPoint's app slot. PaperPaw runs in the partitions it
  finds; where no room for code plugins is left there (the X4), the
  store says so and refuses them.
- The X4 Classic's firmware comes with each release, marked
  experimental: built, not yet tried on a real device. It follows its
  stock firmware where the FreeInk SDK and Papyrix agree: the panel at
  10 MHz with its factory frame rate, its controller as the factory's
  NVS names it (else as it answers, else a UC8279), the charging pin
  high while charging, the card started up to four times, and its
  power kept on through sleep. Its partitions keep the factory's NVS
  whole.
- The Xteink X3's firmware comes with each release too (`cargo x3`),
  experimental and not yet tried on one, without store plugins yet: the
  X4's chip and buttons, its UC8253 panel at 792 x 528 in black and
  white, a fuel gauge and a clock; an emulator profile too. What is not
  known without a device is listed in `shells/xteink`.
- The plugin store has five more groups, so every kind of plugin has a
  place: notes, learning, news, tools and games, before system. Of
  PaperPaw's own plugins, vocabulary moved to learning; news, wallabag
  and the browser to news; readwise and joplin to notes.
- paperpaw-dev checks the group in `store/store.ini` (`package.store-group`).

## [0.5.0-beta.4] - 2026-10-24

A beta for updates over the air, from testing the update to beta.2 on
the X4 and X4 Pro, and for the largest interface text, which every
screen now takes. Code plugins built for earlier betas run unchanged:
the plugin interface stays 2.

### Fixed

- After an update passed its first minute on trial, the X4s restart by
  themselves, so that code plugins (store tools) are there again; before,
  they were missing until the next start.
- A download that got no answer at all (seen on the second connection
  after the update check, until a restart) is tried once more with the
  radio started anew.
- After an update the device no longer asks to make Diagnostics the home
  screen: it no longer binds the home slot, and the plugin manager offers
  a new screen for a slot only when it is bound as high as the one in use.

### Added

- SDK: `UiStyle::noted_row_for` makes a list's rows as high as its
  notes need, up to `NOTE_LINES_MOST` (4) lines, and
  `ListView::set_row` sizes them anew; a row's note shows as many lines
  as its row holds.

### Fixed

- After an update the device no longer asks whether to make Diagnostics
  (or any screen bound only as a fallback) the home screen: Diagnostics
  binds no slot, and a newly offered screen is asked about only where it
  binds the slot as high as the one in use. `Env::slot_screens` lists
  the highest priority first.
- At the largest interface text on small screens the notes of About,
  Keys, News, a catalog's fields, Plugins, Profiles, Cards and Wi-Fi
  show whole.
- After an update over the air the plugins installed from the store
  (statistics, widgets, the syncs) work again by themselves: once the
  update ran its 30 seconds of trial, which go without them, the X4s
  start again, at once or once the book open is left. Before, an update
  that kept the plugins' builds left them off until the next start.
- A download that gets no answer at all (seen with updates on the X4
  and the X4 Pro: connected, then nothing until a restart) is made once
  more after the X4s start their radio anew, instead of failing with
  "The update could not be downloaded". Its log line says where it was
  connected and how much memory was free.

## [0.5.0-beta.3] - 2026-10-24

A beta with fixes from testing beta.2: a press made while a new screen
still refreshes is no longer lost, and more screens fit at the largest
interface text. Code plugins built for earlier betas run unchanged: the
plugin interface stays 2.

### Fixed

- A press made while a new screen was still being refreshed (opening
  Settings on the X4 takes a full refresh) was dropped, so the first
  press after opening a screen sometimes did nothing. The X4s now count
  a screen as shown when its refresh begins.
- At the largest interface text more screens fit: the statistics'
  calendar (its days no longer shrink to dots) and overview, the device
  check, the store's plugin page and source, the Calibre hint, the file
  finder and catalogs, the phone step of Wi-Fi, the statistics' sync
  and the profiles. Store plugins changed so carry version 1.0.1.
- Vocabulary 1.0.1: a card answered Again waits its ten minutes before
  it comes back (also published on its own, between betas).
- A store plugin released on its own is built for the X4 Classic too.

## [0.5.0-beta.2] - 2026-10-24

The second beta: the fixes from testing beta.1, as a new user would use
it, from the web flasher to the plugin workshop. Every plugin now has
a version of its own, so a fix to a store plugin reaches readers as an
update, with a release or alone between releases. Code plugins built
for beta.1 run unchanged: the plugin interface stays 2.

### Added

- Look for updates (Update, off unless chosen, also asked in the
  first-run setup): about once a week the device asks for a newer
  PaperPaw by itself, only while Wi-Fi is on anyway or it charges, never
  waking the radio on the battery. What it finds waits on the Update
  page, its entry in the settings marked New; nothing over reading.

- Every plugin has a version of its own, apart from PaperPaw's: its
  crate's, which its manifest says (`package_version!()`, the built-in
  plugins too), 1.0.0 to start. A release builds a store plugin that did
  not change again for its firmware at the version it has; the plugin
  library adds the build to it. CI fails where a store plugin changed
  since the last release and its version did not (a commit says
  `Plugin-unchanged: <id>` for a change no reader sees).
- A single store plugin can be updated between releases
  (`store-plugin.yml`): built for a release's firmware, signed and
  published as a release of its own, `<id>-v<version>`, which the plugin
  library imports.

- Restart now, last in the settings: asked once, it saves what is open
  as when the device goes off and restarts PaperPaw.
- The first-run setup's pages fit at the largest interface text on the
  X4: laid out closer there, with the buttons side by side where needed.

- A book downloaded from a catalog (OPDS) shows how far it came: its
  percent and a bar where the size is known, the bytes so far where not.
  Back stops the download, and nothing of it is left in the library.
- SDK: `HttpRequest::progress(&event)` and `HttpProgress` tell a plugin
  how far a download came (at most every 5 % or 2 seconds, never after
  the reply); shells report it with `Kernel::http_progress`. Only new
  items: the plugin interface stays 2.

- The emulator answers the requests of PaperPaw's own plugins offline
  with `--network fixtures`: a small plugin library (a data plugin with a
  long summary, screenshots and a source, a code plugin, a long name),
  so that the store's screens can be drawn and checked without a
  network, alike everywhere. On the X4 profiles the pretend radio's test
  network is known from the start, so it is joined when a plugin wants
  the network. Besides the library it answers an OPDS catalog (Project
  Gutenberg's, with books to download), fonts to download, KOReader's
  progress sync, the weather, Wikipedia and a news feed.
- `paperpaw-dev --network fixtures` checks offline with these answers:
  `visual` draws the screens that show them with their data, and `reach`
  takes a screen out of reach whose requests went unanswered for an
  error. `visual.overlap` now also finds a text under a row, a control
  or another text. CI runs `visual` and `reach` so.

### Fixed

- In the first-run setup's Add more step, plugins switched on are added
  also when you leave the page with Back, not only with its Add button;
  the setup's card says how many are being added, then Added.

- A code plugin built for the X4 or X4 Pro with the SDK's `outline`
  feature now stops with a message that says to build for the readers
  without it (it is in a plugin's fingerprint, and the readers draw font
  packs): on the X4 it failed in `once_cell` (no `compare_exchange` on
  its chip), on the X4 Pro it built a file the reader never loads. The
  emulator's build takes it from a target section of the manifest
  (docs/plugins.md, "The crate").

- On the X4 and X4 Pro on battery, clicks made while the screen refreshes
  are no longer lost, and a quick click is no longer taken as a long
  press (which opened the quick panel while turning pages).
- On the X4 and X4 Pro, a click of Right while a page is shown is no
  longer taken for a long press (which opened the quick panel).
- Clicks made while the X4 was busy, up to about a second before it
  showed the next page, count; only a long tail of them is dropped.
- Reading a long book on the X4 no longer stalls for seconds every few
  pages while its pages are counted in the background: a new chapter's
  cache took 4 to 8 seconds, now under one.
- Moving through the library and its shelves on the X4 takes a fast
  refresh of the two rows or covers that changed (some 0.6 s), not a
  frame in grays or a full refresh of the screen (1.7 to 2 s). Covers
  there show in black and white; a setting (Library › Covers) shows them
  in gray, finer but slower to move over.
- On the X4, pictures in gray (the sleeping cat, covers) are as dark as
  on the X4 Pro; they were faint.
- Small changes on the screen (a download's progress, a focus moved, the
  clock) no longer make the whole screen flash every few times: only
  page turns count towards Settings › Full refresh, as it says. A book's
  download refreshes its progress line without a flash, and goes faster.

## [0.5.0-beta.1] - 2026-10-24

This is a beta, for testers and developers. It is the first release of
PaperPaw's firmware for the Xteink X4 and X4 Pro, and it brings much
that is new on every device. Expect bugs, and expect some features to
change before 0.5.0. Keep a copy of your books and notes. When
something goes wrong, please open an
[issue](https://github.com/hthoene/paperpaw/issues). Say which device
and version you use and what you did. On the X4, a crash is written to
`.paperpaw/crashes.txt` on the card at the next start; please add that
file to your report.

### Reading

- Holding a page button goes to the next or previous chapter; *Hold a
  page button for the next chapter* under Controls turns this off.
  *Next chapter* and *Previous chapter* can go on any button or gesture.
- On the X4, the front buttons Left and Right turn pages in a book.
  *Swap the page buttons* under Controls turns them round.
- Holding *Select* while reading sets or removes the page's bookmark.
  *Bookmark* and *Home* are in the reader's bar. *Go to a percentage*
  and *Go to page* are in *More*.
- New status items: the page in the chapter, the pages left in it, and
  a short bar for how far into it you are. *Now reading* shows the
  pages and minutes left in the chapter.
- The status lines have a font of their own: as the menus, as the book,
  or any font installed. They sit closer to the edge, so a page holds
  more text. By default they are calm: the time and the page.
- More control over the page: margins at the top and bottom, space
  between paragraphs and indents as two switches, and line spacing down
  to 80 %. A live preview above the reading settings shows each change
  at once.
- A new text size lays out the open book again at once. *Turn the
  screen* and the full refresh can be a book's own, with *Only this
  book*.
- Text on black and white panels is fuller; *Text darkness* under
  Display chooses light, normal or full. *Smooth text* shows pages in
  four grays with soft letter edges, but pages turn more slowly.
- *Marks in the text* sets how each kind of mark looks: highlights,
  links, search hits and the words plugins mark. Highlights now show on
  black and white panels. *Mark vocabulary* marks the words you looked
  up.
- The dictionary can look a word up on Wikipedia: choose *Online* in its
  panel, and the summary shows beside the dictionary entries.
- Books whose pages go from right to left mirror taps and swipes.
- Font size offers only the sizes the reading font has: on the X4 and
  X4 Pro, whose built-in fonts stop at about 12 pt, no larger ones that
  looked the same. A size set before past them gives way to the
  largest. At the largest, the help says that a font with larger sizes
  gives more, and *Discover fonts* on the reading settings opens the
  font browser (or its page in the store, where it is not installed).
  Scalable fonts keep every size.
- A book that cannot open for lack of memory (on the X4, while Wi-Fi is
  on) says so. Before, the device could restart, or the book opened at
  its start and lost your place. A book whose file is gone says that
  too.

### Books and fonts

- HTML, FictionBook, Markdown and comic books are plugins of their own,
  so each can be switched off or replaced. Markdown files show the
  pictures beside them.
- *More fonts* below the font choice lists the fonts of paperpaw.org's
  font library. Download one and read with it at once. The first are
  Atkinson Hyperlegible Next, Crimson Pro, EB Garamond, Figtree, Fredoka
  and Nunito.
- On the X4, font packs on the card add other scripts. Packs for
  Cyrillic and Greek make Russian, Ukrainian and Greek books, menus and
  keyboards work.
- On the X4 and X4 Pro, pages with pictures, covers and sleep screen
  pictures show in four grays. *Pictures in grays* under Display does
  the same for screens with pictures, such as the library's covers.
- Pictures on the X4s are dithered in a new way: line drawings stay
  clean, and covers and photos keep their tones. Night mode leaves
  pictures as they are, unless *Pictures in night mode inverted* is on.
- Damaged or crafted books, pictures, fonts and dictionaries can no
  longer restart the X4 or use up its memory. What cannot be read is
  left out, and the rest carries on.
- Many fixes in EPUB, FictionBook, plain text, Markdown, comics and XTC.
  Plain text reads UTF-16 files, and file names that begin with a letter
  beyond ASCII ("Ärger.epub") no longer crash the X4.

### The library

- The library is a calm list. The book you are reading comes first, as a
  card. Then come all books with cover, title, author and progress; a
  check marks finished books, and *New* marks books added recently.
- Filters above the books: All books, Reading, Unread, Finished, Next in
  your series, Shelves and Find a book. Folders, favourites, authors and
  series are on the shelves. *Find a book* picks a book by a letter.
- *Show books as: Covers* shows a grid of covers with the progress on
  each; the books of a series form one stack.
- Hold a book for its sheet: go on reading, its series, finished or
  unread, favourite, information, archive and delete. Deleting asks
  first and removes what is kept for the book.
- *Open with* lists the reading apps that read a book. The reading
  settings choose a default app and one per format, and the file manager
  sets one per folder.
- The list of books is kept up to date in the background, never while a
  book is open. Large libraries work on the X4, and the library holds
  next to nothing while you read.
- What is kept for a book moves with it. *Clean up storage* under System
  removes what is left of books that are gone; it also runs by itself
  while the device charges, at most once a week.
- The file manager opens folders of any size a page at a time. Moving a
  folder takes the books' progress and notes along.

### The home screen and widgets

- The home screen is a grid of 3 columns by default, with up to eight
  pages. *Edit home screen* in the main menu edits it, by touch or with
  the buttons. On the X4 Pro, dragging keeps up with the finger.
- Many new widgets: an analogue clock, a word clock, a world clock, a
  calendar, the year so far, a countdown, the moon's phase, a quote, a
  picture, a note, a to-do list, the next book, a series, favourites, a
  highlight, the library in numbers, the battery and PaperPaw's logo.
- The reading statistics add a daily goal ring, a reading calendar,
  books this year and a streak. A weather plugin (Open-Meteo) adds the
  weather now and a forecast; nothing is sent before you choose a place.
- Every widget has settings of its own: what it shows, its style, size,
  alignment, spacing and frame. The same widget can be placed more than
  once. *Widget look* and *Frames of widgets* change all widgets at once.
- The clock has clearly different sizes: small, medium (the default),
  large, and as large as fits. On the X4s a large clock is truly large.
- The home screen shows no focus until the first press; *Show the focus
  at once* brings the old way back. Fewer columns or rows no longer
  leave a page empty.

### The sleep screen

- By default the sleep screen shows a sleeping cat, drawn for PaperPaw.
  The cover, pictures, a message and the other choices remain.
- *The page with a picture*: the page you read stays, and a picture or a
  widget goes over it. You choose where it sits, its size and how much
  of the page shows through.

### Menus, settings and the look

- A new look, rounder and friendlier: soft corners, pills for buttons
  and switches, plain headers, faint lines between rows and margins of
  4 mm. The menus' font is Source Sans 3. Empty screens say what belongs
  there and how to fill it.
- With advanced settings, Appearance sets the header, corners,
  highlight, lines between rows, cards and the size of titles, margins
  and rows. Themes are listed under *Theme*; the Pebble theme comes
  switched off.
- No label is cut any more. Long titles and button words get a little
  smaller or wrap onto a second line, at every text size and language.
- Hints: the first time a screen opens, a short note says what its
  controls do. *No more hints* switches them off, and *Show hints again*
  under System brings them back. Every plugin also says how it works, in
  the plugin manager and in the store.
- On touch screens, numbers you set by eye are sliders. A slider moves
  with the finger from where it was and never jumps. With the buttons,
  a changed value shows at once; *Select* keeps it and *Back* goes back.
- The quick panel and the reader's panel have wide sliders and larger
  buttons on touch screens. The front light is there now, no longer in
  the main menu.
- Under Settings, Plugins, *Screens*, you choose which plugin shows each
  screen: the home screen, the library, the reader, the lock, the
  keyboard, the sleep screen and more.
- Each settings page ends with *Reset to defaults*, the main page with
  *Reset all settings*. A setting with two choices switches at once, and
  moving past the last entry of a list goes round to the first.
- *Device name* under System names your reader for Calibre, the progress
  sync, Wi-Fi transfer and your router. *First day of the week* sets the
  start of the week for the statistics and calendars.
- New status items: *Battery when low* shows the battery only below a
  level, and a Wi-Fi sign shows while the device is in a network.
- A plugin's settings group that names no place on the settings' main
  page is listed at the end of *Customise*, by name, with a plugin's
  icon, instead of first among the books' groups.
- No explanation is cut short any more. Every setting's description fits
  the two lines of its row on the X4 and the X4 Pro, in English and
  German, and a test keeps it so. The text below *Progress sync*'s rows
  takes the room there is; where it still has none, a row *More about
  this* shows it whole.
- First-run setup: the sample of the font size is in the book's font,
  at the size chosen, as the reader shows it: no longer in the
  interface's font, its second line cut. The card grows with the size
  (fewer words where there is no more room), and follows the size as it
  changes.
- Every step of the text size changes the text, in the setup, the
  settings, the quick panel and with the reader's buttons: sizes the
  font of the X4 and X4 Pro draws alike (9 and 10 pt on the X4 Pro) are
  skipped. Sizes kept stay as they are.

### Touch, buttons and the keyboard

- The quick panel by the buttons: hold Right (on the X4), or choose
  *Quick panel* at the top of the main menu. The tips on the home screen
  and in a book say so.
- A number in the first-run setup and in a menu is changed with the
  buttons as on the settings pages: Select starts, left and right change
  it, Select keeps it, Back goes back to the value before, and the hint
  bar says so. Left and right no longer stay on a number in the setup,
  and the quick panel's sliders name Keep and Cancel too.
- The keyboard on touch screens has round keys and takes less of the
  screen; *Shape of keys* makes them a little taller or wider.
  *Keyboard size* (Small, Medium, Large) makes it a quarter lower or
  higher: with round keys about 30 %, 40 % or 50 % of the screen, never
  keys below 4 mm. German has ten keys a row: hold a, o, u or s for ä,
  ö, ü and ß, or switch on *Umlaut keys*. On narrow screens, a row too
  wide is split in two. The keyboard takes up to 2,000 characters.
- Touch lock: *Lock touch* in the quick panel and the reader's menu.
  Taps and swipes do nothing, and the buttons still work. To unlock, tap
  twice and keep the finger down, or hold the power button or the home
  key on the X4 Pro. However it was switched on (the quick panel, the
  reader's menu, Settings, a button), it says how to unlock when it
  turns on, and says when it is unlocked. While it is on, a tap or a
  swipe says it again, at most every ten seconds and calmly, never
  between the steps of the way out. It works on slow screens too: the
  two taps may be up to half a second long and a second apart, and a
  hold unlocks as soon as it is long enough if the finger moves a
  little.
- On the X4, a row above the buttons names what each does; *Show what
  the buttons do* under Controls switches it off.
- Buttons answer better: a press counts as soon as it holds, and presses
  made before a new screen shows are dropped. *Button response: Faster*
  reads the buttons more often, for a little more battery.
- On the X4, *Buttons and gestures* refuses a change that leaves no
  button to select or go back with. *Reset buttons and gestures* takes
  every change back.

### Tools

- Flash cards, in the vocabulary tool, with spaced repetition (FSRS,
  Anki's scheduler). Edit decks with your phone, also from CSV or an
  Anki deck. Words you look up can become cards, and a card due can be
  the sleep screen.
- Reading goal: a daily goal for the reading statistics, and a cat whose
  mood follows today's reading. The *Reading cat* shows it on the home
  screen or as the sleep screen.
- Joplin: a book's highlights and notes become a note in Joplin, through
  Joplin Cloud, Joplin Server or WebDAV. It is off until you want it.
- Battery use records the battery while the device is awake. It shows
  the use per hour and per 100 pages, how long the battery lasts, and
  which plugins kept the device busy.
- A guided device check in the diagnostics tool tests every part of a
  device, one page at a time, and writes a report to the card.

### Wi-Fi, sync and the store

- Wi-Fi transfer opens in the library folder (there from the first
  start) and says where uploads go; books sent elsewhere are pointed to
  the library, gently. Files and folders can be moved and renamed there,
  their books' progress and notes going along, and each file sent shows
  how far it is, can be tried again alone, and is in the list at once.
- Wi-Fi goes off as soon as you are back home or in a book and nothing
  uses the network, not a minute later. On the X4 this frees its memory
  before a book opens: a book opened right after the store could run
  out of memory. On the X4 a download also takes less memory at once.
- The store tells betas apart: a plugin's versions are compared whole,
  pre-release included (`0.5.0-beta.2` after `0.5.0-beta.1`, `0.5.0`
  after both), as firmware updates are. A newer beta is offered only
  where the device runs a beta or takes betas (*Beta updates*). Its
  message for code built for another plugin interface or compiler says
  so.
- The store marks PaperPaw's own plugins *By PaperPaw*, on their row
  and their page. The mark is taken only from PaperPaw's own library
  (paperpaw.org, over HTTPS), and for a plugin with code only where its
  key is PaperPaw's: a list from elsewhere cannot give it.
- A code plugin's version is its `plugin.ini`'s, everywhere: the
  plugin manager, the store's updates and the transfer's list all go by
  it. Code whose manifest names another version runs at `plugin.ini`'s,
  with a warning in the log; `paperpaw-dev package` finds it before.
- Wi-Fi on the X4 and X4 Pro: the radio is off while you read. It comes
  on when something needs the network, joins the strongest saved
  network and goes off a minute after the last use. HTTPS works, with
  the certificate checked. The X4 sets its clock once online.
- Set up Wi-Fi with your phone: the reader opens a network of its own
  and shows QR codes, and you pick your network and type its password on
  the phone. Only secured networks are joined by themselves.
- Wi-Fi transfer and Calibre work on the X4 and X4 Pro. *Without a
  router* on the transfer screen opens the reader's own network. A
  scan that finds no saved network is made again, up to three times,
  before the device says there is none.
- Calibre: a burst of books no longer stalls the connection as often,
  a connection Calibre stops answering ends after 45 seconds, and the
  device tells Calibre how much its card really has free.
- The Wi-Fi transfer installs plugins from a computer too, such as
  one's own build. The version before stays until the new one is whole,
  and a plugin with code is installed only after you say yes on the
  device.
- In Offline mode, the store, catalogs, Calibre and Wi-Fi transfer offer
  to go online once, instead of failing.
- A plugin store on the device: *More plugins*, in the tools and in the
  plugin manager, with search, groups, sorting and *Update all*. Code
  plugins come from it too, signed by their author; an update signed by
  someone else needs your consent. The store updates only the plugins
  it installed: one put in place otherwise (the Wi-Fi transfer, a card,
  one's own build) is local, and the plugin manager says which is which.
- The firmware is lean: many plugins, such as the statistics, widgets,
  Calibre, KOReader sync, weather and rules, come from the store instead
  of being built in. This leaves more memory.
- A first-run setup guides you through the language, the buttons, the
  text size, the time zone, Wi-Fi, the keyboard and the screen lock.
  *Add more* offers four plugins most readers add.
- Progress sync (kosync) fetches the position on the server before it
  sends its own, so a further position from KOReader is kept. It no
  longer keeps the radio on while you read.
- KOReader's notes exchange, the statistics sync, OPDS, Wallabag, news
  and Readwise no longer delete anything when an answer or a file cannot
  be read.

### Power and battery

- Power modes: Normal, Saving, Offline and Automatic, in the quick panel
  or under Settings, System, Power. Automatic goes to Saving below 20 %
  and to Offline below 10 %. *Network per plugin* sets the network for
  each plugin.
- Quiet reading: background work no longer shows notices over your book,
  and it waits for a good moment, such as when the radio is on anyway.
- The X4 and X4 Pro save much more power: the processor sleeps on
  battery when there is nothing to do, the radio sleeps between beacons,
  and the panel's power goes off after a refresh.

### Devices and updates

- On the X4 and X4 Pro, a secure (HTTPS) connection takes about 15 KB
  less memory and the firmware about 180 KB less flash: mbedTLS is now
  built with the firmware, with only what its client uses and 4 KB for
  what it sends.
- Transfer over USB, further: a plugin's files are put together in a
  folder of the cable's own and installed only if they are those
  offered, each with the SHA-256 offered; the cable and the open Wi-Fi
  transfer never send the same plugin at once. Offers are read a line
  at a time (up to 64 files and 64 books each), and only closing the
  question refuses an offer.
- The Xteink X4 Pro is supported: its display, touch screen, buttons,
  card, battery gauge, clock chip, front light with warmth, sleep and
  Wi-Fi. Its front light dims evenly in single percents.
- The X4 is much faster: a page turns in about 0.7 s and the library
  opens in under 2 s. The screen can be turned.
- Updates from the internet: *Search online* on the update page finds a
  newer PaperPaw, and *Download* fetches it. Updates are signed, and one
  that fails early in its start is undone.
- An update waits while code plugins in use have no build for it yet.
  The update page names them and offers *Keep waiting* or *Update
  anyway*.
- Releases carry signed firmware images of the X4 and X4 Pro, which
  paperpaw.org offers for flashing in the browser.
- Safe mode: after two starts that fail, PaperPaw starts without code
  plugins. Holding *Back* on the X4 or *Home* on the X4 Pro at power-on
  does the same.
- QuireInk is now called PaperPaw. Its settings, statistics and
  passwords are taken over once, at the first start.
- Android: a beta of the app comes with each release, as an APK. It is
  now `org.paperpaw.app` and installs beside the old one. It runs on
  devices with 16 KB memory pages, and on Android 8 to 10 it can write
  to the shared storage.
- Transfer over USB on the X4 and X4 Pro: a computer at the other end of
  the cable installs a plugin or adds books. The device shows what is to
  come, with names and sizes, and writes nothing until you allow it,
  never over a book.
- Betas: *Beta updates* in the update's settings, off at first, asks
  for betas; a device on a pre-release gets the next anyway, and the
  device takes a pre-release only so, whatever the server answers.
  Betas come in order before their release (`0.5.0-beta.2` after
  `beta.1`, `0.5.0` after both) and show calmly, as `0.5.0 Beta 1`.
  Android's version codes follow the same order (of 1.2.3: beta.1 to
  beta.49, then `-dev`, then rc.1 to rc.48, then the release), and a
  version they cannot order is refused.
- The X4 keeps less in memory from the start: ids take two words
  instead of three, translations index their messages in two bytes
  where they fit, and actions are not kept twice. Reading an EPUB peaks
  some 24 KiB lower.

### Your data and security

- PaperPaw never writes over a file it cannot read, such as
  `paperpaw.ini`, passwords, notes, statistics, profiles or rules.
- Writes are safe against power cuts: a file holds its old content or
  its new one, whole, also on the X4's card, where a file moved or
  renamed is at its old place or its new one, never at neither. Going
  back to an older update no longer writes over data that a newer
  PaperPaw wrote.
- An audit of everything PaperPaw keeps found ways it could lose notes,
  places, statistics, cards or passwords; each is closed, with a test.
- A web request printed for debugging (in a log or a report) shows no
  password or key: not the values of its headers and params, not its
  body, not the query of its address.
  Among them:
  - *Clean up storage* removes by itself only what can be made again.
    The notes and places of books not found go only when you agree.
  - A reading place that cannot be read is never written over with the
    start of the book; it is kept aside and you are told.
  - Highlights and notes in EPUB books keep their place when the book's
    own styles are switched on or off.
  - Progress sync, KOReader's notes and the statistics sync delete
    nothing on a failed fetch or an unclear file, and ask where one side
    lost everything.
  - Lines PaperPaw does not understand, from a newer version or written
    by hand, are kept. An old copy of the passwords goes only once the
    passwords file holds it.
  - Removing a profile, a password or a vocabulary word asks first;
    *Reset to defaults* keeps folders, accounts and passwords; the file
    manager protects PaperPaw's own folders.
- While the device is locked, nothing shows above the lock screen but
  what the lock opens itself. The lock's code is kept with a random
  salt.
- A redirect no longer takes a password to another server, nor goes
  from `https` down to `http`. Calibre connects only to a computer you
  chose, and a plugin cannot change another plugin's passwords.
- A book's position and annotations say which book they are of by its
  content, its size and KOReader's partial MD5, beside the hash of its
  path they are named by (`library::Fingerprint`, `positions::book`,
  `annotations::book`). The reader takes it once when it opens a book
  (12 reads of 1 KiB at most) and keeps it with the position
  (`positions::save_with_book`) and the annotations
  (`annotations::stamp`, a line `# book <size> <md5>`). Both files keep
  their versions: PaperPaw of before reads them as before.

### For plugin authors

Plugins written against 0.3 need changes: the lists below name what
replaces what. A code plugin is a file bound to the plugin interface
(`native::INTERFACE`, now 2), and is rebuilt only when a release raises
it. `crates/paperpaw-sdk/api.txt` lists the whole public API, and
[Designing the plugin SDK](docs/sdk-design.md) says how it is kept from
breaking again.

#### Breaking SDK changes

**Handlers, commands and replies**

- Every handler queues its commands in `Commands` instead of a
  `&mut Vec<Command>`: actions, events, observers, work steps, voices,
  widgets' input and the installer. It has `push`, `extend` and `emit`
  and reads as a slice. Its fields are private, so it can carry more
  later without a new signature. A closure such as `|env, out|` stays as
  it is; a parameter typed `&mut Vec<Command>` becomes `&mut Commands`.
- Actions have one shape, with params everywhere. A handler is
  `.handler(|env, params, out| ..)`; `handler_with` and
  `ActionParamsHandler` are gone. `Command::RunAction(id, params)` takes
  the place of `RunActionWith`, and `Command::run(id)` runs one without
  params. A screen reads them with `cx.params()` in `on_action`.
- Commands stay small, 88 bytes in place of 216 (a test keeps
  them so): `Command::Http`, `Speak`, `Respond`, `Broadcast`, `Work`
  and `SetSecret` hold their payload boxed, made with `Command::http`,
  `speak`, `respond`, `broadcast`, `work` and `set_secret`. `HttpRequest`,
  `Speech`, `ServerResponse` and the new `Broadcast` keep their fields
  to themselves: they are made with their builders
  (`HttpRequest::param` is new; `Speech::in_language`, `at_rate`,
  `with` and `queued` take the place of `language`, `rate`, `params`
  and `queue`) and read with methods.
- Replies are read typed only. The reply to `Command::Speak` is a
  `Spoken` (whether it was spoken, and the speech's parameters); the
  string keys of replies (`http_params`, `server_params`,
  `socket_params`, `speech_params`) are the kernel's. `Speech::new`
  takes an `Event<Spoken>` (or an id).
- An HTTP answer's body is bytes as they came (`HttpReply::body`
  is a `Vec<u8>`, `text()` reads it as text), no longer decoded with
  losses, and it reaches the plugin's handler without a copy: an event
  may carry its data as it is (`Params::carrying`,
  `EventData::into_params`), which observers do not see. A failed
  request says why (`HttpReply::error`, an `HttpError`: no network,
  DNS, TLS, timeout, refused, replaced, cancelled, I/O), and a body cut
  to the most the request takes says so (`truncated`). `EventData`
  types are `Clone`.
- Signatures are numbers, made without allocating: a text
  filter's and a status line's signature returns a `u64` (0: does
  nothing), like a decorator's. `digest::Fnv1a64` makes one, fed piece
  by piece, as a `Hasher` or with `write!`. A menu item's label and badge
  are a `Cow<'static, str>`, so a fixed text costs nothing.
- The registrar's methods take ids as `impl Into<Id>` and text as
  `impl Into<Cow<'static, str>>` or `impl Into<String>`, so `&ID`, a
  setting or an event and a `&'static str` or `String` go in as they
  are. `Id`, `Setting` and `Event` are `AsRef<str>`.
  `SettingExtra::follows` and `shown_if` name the setting by its `Id`.

**Settings and services**

- Settings are declared as `static` lists and take next to no
  memory. `SettingSpec` has one builder set, all `const`: `switch`,
  `number`, `choice`, `text`, `language` and `list` take the id, title,
  default and group, and `SettingSpec::new` takes any kind and an `Id`
  made at run time. `description`, `advanced`, `scoped`, `guarded` and
  `kept_on_reset` are `const` too. Units, steps, sections, sliders,
  editors and secrets come from a `static SettingExtra` (`with_extra`);
  `with_owned_extra` is for settings made from data. Gone: `fixed`,
  `fixed_switch` and the like, `described`, `for_advanced`, `group` and
  the builders that set an extra at run time. The kernel keeps each list
  given to `add_settings` once, not each spec, and the network settings
  it makes for every plugin share one extra
  (`SettingExtra::section_of_plugin`). On the X4, settings and groups
  kept so make the heap's peak while reading 11.8 KiB lower.
- `SettingGroupSpec`'s builders are `const`, and
  `add_setting_group` takes a `&'static SettingGroupSpec`, kept by
  reference, or one made at run time. The kernel's groups and the
  plugins' with a constant id are statics.
- Services have one path, by their contract: a service type names
  its key with `service_type!` (`ServiceType` is an `unsafe` trait, the
  key the promise that every type naming it is this one), is provided
  with `r.provide(id, value, version)` and found with
  `env.services().get::<T>(id, min_version)`. A trait object is wrapped
  in a struct of the plugin's own (`struct Weather(Box<dyn Forecast>)`).
  `publish`, the lookup by type name, `get_typed` and `get_versioned`
  are gone; a library of one's own is a `library::service::LibraryService`.
- What was deprecated is gone: `Registrar::add_service` and
  `Contribution::Service` (`Registrar::provide` offers a service),
  `Service::any` and `Id::placed` (`Id::unplaced`).

**Env, and what only the kernel uses**

- `Env`'s parts are read through methods (`env.settings()`,
  `env.i18n()`, `env.now()`, one for each part), so what it carries can
  change without a change to plugins. Its fields are private.
- `EnvParts`, `WorkStates::set_works`, `WorkStates::set_note` and
  `ScreenKey::new` come with the `kernel` feature: only the kernel and
  tests make an `Env`, set work states or number screens.
- Cleaned up before the first release (installers): what only the
  kernel and the plugins that install do comes with the `kernel`
  feature, the reading parts staying for every plugin. `native`:
  `remember_author`, `remember_code`, `remember_fetched`, `fetched_for`,
  `keep_code_room`, `installed_code_need`, `check_file`,
  `CROSSING_CRATES`, `LAYOUT` and `CodeRoom`'s constructors (`known_author`,
  `installed_code`, `code_room` and `CodeRoom`'s reading stay);
  `install::local` (`Staging`), `install::marks`, `remember_store_install`,
  `forget_store_install` and `adopt` (`origin`, `Origins` stay);
  `firmware::Update` and `FirmwareCommand::install`, the one way to ask
  for an install (`FirmwareCommand::Install` is matched, not built);
  `data::take_over_old_name`; `Secrets::encode` and `decode`;
  `library::save`, `catalog::write` and `Index::save`.
- `Contribution` is the kernel's: a plugin adds through the
  `Registrar`'s methods, and `Registrar::add`, `contributions` and
  `into_contributions` need the `kernel` feature (a plugin's tests that
  look at what it adds name it as a dev-dependency).

**Names, and types that may grow**

- One name per item of the core API. The hints have each their
  own: `help::HelpHint` (was `help::Hint`), `HintVisibility` (was
  `Hints`, when a screen shows the row of button hints), `ui::look::
  HintRowPart` (was `HintsPart`) and `widgets::CellSpan` (was
  `CellsHint`); `Hint` is the label of one button and `ActionHint` an
  action's. The three modules of sections have their own too:
  `builtin::menu_sections` (was `sections`), `settings_page` (was
  `settings_sections`) and `group_sections` (was `setting_sections`).
  `replies::FolderChosen` is the answer to `ask::folder` (was
  `Chosen`; `Picked` stays the answer to `ask::choice`).
- Types that may grow are `#[non_exhaustive]`, with constructors
  where plugins build them: `decorations::{PageWord, PageText,
  Decorated}::new`, `semantics::Part::new` and `Described::new` (with
  `.focused()`, `.on()`, `.clip()`); also `decorations::Mark`,
  `RowLines`, `SliderParts`, `PickerPart`, `Offer`, `Padding`. A match
  on `Offer` needs an arm for answers to come. `Header` and `Pager` keep
  their state: `area()` reads it, `Header::note_focused()` and
  `note_button()` set the note.
- More types that may grow are `#[non_exhaustive]`: `HttpReply`
  and `FolderChosen` (with `new`), `ServerResponse`, `SocketRead`, `MenuItem`,
  `help::{Input, Zone, HelpHint}` and in `work` `WorkState`, `Retry`, `Why`,
  `Refusal`, `WorkRefused` and `Note` (with `new`), `Step` and
  `Importance`. A match on them needs an arm for what comes later.
  `Handled` is exhaustive: an event is handled or not, and a plugin
  matches both.
- Cleaned up before the first release (platform): the types that
  grow are `#[non_exhaustive]`, with constructors where plugins build
  them: `DirEntry::new`, `files::Entry::new`, `LocalTime::new` (its
  weekday from the date), `MemoryUse::new` and `with_stack`,
  `PluginUsage::new`, `WifiNetwork::new`, `Firmware::new` with `keys`,
  `room`, `undone`, `state` and `plugins`, `BookData::new` and
  `SecretChange::changeable`; `install::Digest::text` writes a digest as
  a job keeps it. A match on `WifiCommand`, `PowerLevel`,
  `served::Access`, `install::Digest`, `FirmwareCommand` or
  `BookDataKind` needs an arm for what is to come. `Usage::refreshes`
  and `PluginUsage::refreshes` name their modes (`usage::Refreshes`:
  `fast`, `partial`, `full`, `total`) in place of an array.

**Data, time and storage**

- Cleaned up before the first release (data): the writers of data
  say why they did not write, `Result<(), data::Error>` in place of a
  `bool`: `annotations::save` and `add`; `annotations::set_note` and
  `remove` give `Ok(false)` where there was nothing to change;
  `library::save`, `Index::save`, `install::marks::write` and
  `catalog::write`. `data::Error` takes a `StorageError` (`?`).
- A storage tells when a file was last changed: `DirEntry::modified`
  (`with_modified`) and `Storage::modified`, `None` where it does not
  tell; the memory card's FAT and the computer's file system say it.
  `MemoryUse` tells the largest block the heap could give now
  (`largest_free`) and the heap's regions (`MemoryRegion`: the X4 Pro's
  internal RAM and PSRAM). `read_file` takes its buffer fallibly: a file
  the heap cannot take now is `StorageError::TooLarge`, not the end.
- `time::UnixTime` is a moment (milliseconds since 1970, UTC):
  `from_secs`, `from_millis`, `secs`, `millis`, `now(env)`, `local(offset)`,
  and the ISO 8601 and HTTP date helpers as its methods (`iso8601`,
  `parse_iso8601`, `parse_http_date`) in place of the free functions
  `time::iso8601`, `time::parse_iso8601` and `web::http_date`, which
  counted in different units. `LocalTime` has an `offset` (seconds ahead
  of UTC; 0 where made without a zone), `LocalTime::from_unix` and
  `LocalTime::unix`, and `time::local_time` fills it from the chosen
  zone. `time::unix_seconds` stays, as `UnixTime::now(env).secs()`.
- `Version` keeps a pre-release (`0.5.0-beta.2`) and orders versions
  as SemVer does (`Version::pre_name`, `pre_number`, `with_pre`,
  `release`, `try_parse`), in 8 bytes: a pre-release is `alpha`, `beta`,
  `dev`, `pre` or `rc` with an optional number up to 8190. Any other
  is refused, saying so: by `Version::try_parse` (`VersionError`), by
  the data plugin loader's warning, by `paperpaw-dev`'s package check
  and, for a code plugin's `package_version!`, by the build. So a plugin
  of a beta loads and a later beta is an update; `satisfies` goes by the
  numbers. `SDK_VERSION` is the crate's version, a pre-release between
  releases (`0.5.0-dev`); `firmware::compare_versions` orders by
  `Version` where it can.
- `Manifest` no longer names the SDK it was built against
  (`Manifest::sdk` and the field are gone), and the kernel no longer
  refuses a plugin for it: a plugin with code is loaded by its file's
  fingerprint and the plugin interface, a data plugin by its `formats`.
  `install::fits_in` asks the kernel's version.

**The interface's building blocks**

- Cleaned up before the first release (UI): `ui::fill` and
  `ui::HOLD_ICON` are gone (`Canvas::fill`, `icons::HOLD`);
  `ui::PAW_ICON` is `icons::PAW`. What only the kernel uses is no longer
  part of the plugin API: `ui::draw_button_hints`,
  `ui::button_hints_height`, `semantics::parts` and the constructors of
  `PageLayers`, `Decorators` and `Widgets` come with the `kernel`
  feature, and `Canvas`'s drawing internals are hidden. A cover drawn
  small is `ui::draw_small_image`; whether the device has a row of
  button hints is `env.caps().button_row`.
- `canvas.describe(role, text, area)` says more by its builder
  (`.focused()`, `.on()`, and `.disabled()` for what does nothing now),
  in place of five arguments; parts (`Described`, `semantics::Part`)
  tell whether they are disabled, and `canvas.disabling(true)` tells
  everything drawn into it so. Faint menu and quick panel entries are
  told as disabled, and the remote control's `describe` (and the web
  emulator's) says `disabled` in a part's state.
- Which page of how many a list shows is a `ui::Paging` (`page`,
  `pages`, `is_first()`, `is_last()`), in place of a pair of numbers:
  `Rows::paging()`, `Sections::paging()`, `ListView::paging()`, and what
  `Header` and `Pager` draw and hit.
- Dates and times as values: `time::Date` and `time::Time`, which
  `ui::DatePicker` and `ui::TimePicker` take and give; `line_at` of a
  page's layers and the widget grid's `cell_at` take a `Point`.
- `UiStyle` keeps its fields and is read with methods: sizes in
  pixels as `u16` (`row()`, `margin()`, `gap()`, `line()`, `corner()`,
  `header()`, `finger()`, `control_row()`), text sizes as `f32`
  (`text()`, `small()`, `title()`, `head()`), and its looks
  (`focus()`, `separators()`, `cards()`, `pill()`, `strong()`,
  `header_kind()` with `HeaderKind::{Light, Dark, Plain}`). How many
  grays the panel shows is `black_and_white()` or `smooth_grays()`. A
  style changed for a screen of its own is made with `with_row`,
  `with_margin`, `with_corner`, `with_pill`, `with_text` and the like.
- One path and one name per item of the UI. The types of the
  modules `ui::slider`, `ui::sections`, `ui::reorder` and `ui::offline`
  are reached through them (`ui::slider::Slider`,
  `ui::offline::OfflineOffer` …). The widgets' look is in `widgets` itself:
  `WidgetLook` (was `widgets::look::Look`, apart from `ui::look::Look`),
  `widget_look()`, `TextSize` (was `Size`; `SizeClass` is a widget area's
  class, `TextRole` the interface's text), `choice_setting()` (was
  `choice`), `WidgetStack` (was `widgets::Stack`, apart from
  `ui::Stack`). `ui::Grid` is `ui::CellGrid`, apart from the widgets'
  `grid`.
- `ui::choice::open` and `GridView::new` are gone (`ask::choice`,
  `GridView::asking`).
- `ui::Ui`, what a screen draws with: the canvas, the environment
  and the style together (`Ui::new(canvas, env, &style)`, read with
  `canvas()`, `env()`, `style()`), the interface's parts its methods
  (`ui.button(..)`, `ui.message(..)`, `ui.focus(..)` …). It borrows what
  it is given and allocates nothing.
- `ui::Label`, one builder for the interface's text:
  `Label::new(text).role(TextRole::Small)` (or `.px(size)`), `.ink(..)`
  or `.color(..)`, `.align(widgets::Align::Center)`, `.min(size)`
  (smaller before shortened), `.lines(n)` or `.wrap()`, `.font(..)` for
  a font of its own; `width`, `height`, `draw(&mut ui, area)` and
  `draw_from(&mut ui, x, area)` (on one line from an `x` between
  pixels). It borrows its text and allocates only the words of a line it
  shortens.
- The interface's parts draw with a `ui::Ui`: `Chips`, `Segments`,
  `Dialog`, `Header`, `Pager`, `TextView`, `DatePicker`, `TimePicker`,
  `offline::OfflineOffer` and `reorder::Reorder` take `&mut Ui` in place
  of canvas, environment and style, and the rows `ListView`, `Rows` and
  `sections::Sections` draw are given the `Ui` (`|ui, index, rect,
  focused|`; its canvas is `ui.canvas()`).
- What `ui::Ui`, `ui::Label` and `ui::Row` draw is drawn only
  through them: `draw_button`, `draw_faint_button`, `draw_icon_button`,
  `draw_focus`, `draw_separator`, `draw_heading`, `draw_switch`,
  `draw_message`, `draw_note_line`, `draw_empty`, `draw_card`,
  `draw_progress`, `draw_row_icon`, `draw_header`, `draw_text_field`,
  `draw_value_row`, `draw_noted_row`, `draw_switch_row` and the rows'
  heights are the SDK's own (`ui.button(..)`, `Row::value(..)` …);
  `draw_number` is gone (`Row::number`). Measuring without a `Ui` stays
  with the functions (`ui::message_height`, `heading_height`,
  `icon_room` …), and the `Ui` lost its copies of them and of
  `focus_shown`. `draw_label` stays for drawing without a style (the
  widgets).
- `ui::Row`, a row of a list as a value: `Row::value(title, v)`,
  `Row::plain(title)`, `Row::switch(title, on)`, `Row::number(title, v,
  room)` (or `Row::of(title, RowKind::..)`), with `.note(..)`,
  `.focused(..)` and for numbers `.step_focus(..)`. The one value gives
  `height(env, style, width)`, `draw(&mut ui, rect)` and `hit(..)` (a
  `RowPart`: the row, its switch, its `−` or `+`), so that they cannot
  disagree. `RowKind` and `RowPart` may grow.
- One input contract for the building blocks. Those that take the
  buttons' actions and gestures (`on_action`, `on_gesture`) answer with
  a `ui::Response`: `Ignored` (the screen's), `Same`, `Changed` (draw it
  again; `request` asks for it) or `Done` with a result. `TextView`,
  `Dialog` (`on_scroll_action` is `on_action`), `DatePicker` and
  `TimePicker` (`on_tap` is `on_gesture`), `NumberEdit` (`Done` with
  whether the value was kept) and `offline::OfflineOffer` (`on_tap` is
  `on_gesture`, `Done` with the `Offer`).
- `ui::focus_shown(env)` says whether a focus is shown: always
  without a touch screen, with one once the buttons were used last. The
  kernel starts with `Env::last_input` at `InputMode::at_start` (`Touch`
  on touch screens). The SDK's parts and the plugins ask it rather than
  the device.
- `ui::Chips` and `ui::Segments` keep their labels: they are given
  once when the part is made (`Segments::new` takes the labels, not
  their count) and `draw` no longer takes them; `label(index)` reads one.
  A label is a translation key (or text that is none), kept as a
  `Cow<'static, str>` without a copy and translated where it is measured
  and drawn.
- A look's painters take a `ui::look::PaintCx` (the canvas and the
  style, more to come) and the part: `fn(&mut PaintCx, &SwitchPart) ->
  bool`.
- The interface's colours come from the palette: `faint(Ink)` and
  `focus_faint(focused, Ink)` take a role (`Ink::Note`, the new
  `Ink::Detail` for what is told beside a row's title, `Ink::Muted` …)
  in place of a gray; the focus and the header use the palette's text
  and paper. `Palette` holds colours (`color(ink)`), `Ink::ALL` is a
  slice, and `Ink::CardPattern` is gone (cards on black and white
  panels are a sparser pattern of `Ink::Card`). The palette setting
  names `detail` too.

**Data plugins**

- Data plugins of formats 3 (not yet released) are written alike in
  every file. A list is a key a line: an action's steps are a `run`
  line each, a line one step with its commas, so a value with a comma
  needs no escape (`entries` and `hide` of `menus.ini` still read ids
  separated by commas). `notify:` takes a key of the plugin's
  translations and says its text in the reader's language. In
  `menus.ini`, `[menu <id>]` is another plugin's menu (`add`, `hide`)
  and any other section a menu of one's own (`title`, `entries`);
  `add` and `hide` in a plain section are still read. `paperpaw-dev
  check` says where a file is written the earlier way
  (`package.actions`, `package.menus`). The ini files take a section
  name of words with single spaces between them.

#### New

- The SDK's design rules ([Designing the plugin SDK](docs/sdk-design.md)):
  how it grows without breaking plugins. Tests check four of them over
  the crates whose types cross: growing types are `#[non_exhaustive]`,
  handlers take no `&mut Vec<Command>`, nothing is `#[doc(hidden)]`
  outside the `kernel` feature, and no deprecated item reaches a
  release. What they found before is listed and may only get less.
- Code plugins are bound to a plugin interface, not to a release
  (ADR 0026): a plugin file runs on every PaperPaw of its interface
  (`native::INTERFACE`, 2, in the fingerprint's top bits), betas and
  releases alike, and authors rebuild only when a release raises it. The
  SDK and `paperpaw-native` have no build scripts, so a plugin built on
  Windows is the same as one built on Linux (`tools/reproducible-plugin.py`,
  in CI on both); a plugin project keeps `paperpaw-plugin.x` beside its
  `.cargo/config.toml`. CI fails where what plugins see changes without
  the interface raised, and by default any change raises it: the public
  API and the shape of the types (fields, variants and trait items in
  order, private ones too, functions' pointers with their ABI) and the
  values of the constants of the SDK and every crate of PaperPaw below
  it, with every feature on (`api.txt` and `shape.txt` of each,
  `tools/interface.py`), and the crates of others whose types cross,
  each pinned to one version. Only a new type, a function outside a
  trait, a constant or an `impl` goes free.
- A code plugin's file names the compiler that built it (format 2 of
  `.ppn`, taken by `paperpaw-sign pack` from `.comment`, which
  `paperpaw-plugin.x` now keeps). The X4s, the store and an update's
  fetch take only files of the firmware's fingerprint and compiler
  (`Native::fits`), the emulator only libraries of its own compiler; an
  update answer may name both (`abi`, `compiler`). Files of format 1 are
  refused as built for an older PaperPaw. The repository's
  `rust-toolchain.toml` pins the release's compiler, 1.99.0. New in the
  SDK: `native::compiler` and `firmware::beta`.
- Native code plugins as files (`code.ppn`): the X4 and X4 Pro load
  them from the card at start. `paperpaw-sign pack` makes and signs
  them, and `samples/hello-native` shows how.
- Data plugins of formats 3 add menus, actions, icons, setup steps and
  fonts (`menus.ini`, `actions.ini`, `icons.ini`, `setup.ini`,
  `points/*.txt`), and mark words with `marks.ini`.
- `ui::NumberEdit` changes a number setting with the buttons as
  the settings pages do (Select, a preview while it moves, Keep or
  Cancel), with its hint bar (`ui::editing_hint`).
- `library::tell_moved` tells the plugins that a file or folder moved
  (its books' progress, notes and statistics go along, settings holding
  the folder follow), as the file manager and Wi-Fi transfer do;
  `library::moved_folder` gives a folder's new path after a move.
- `ui::shown_step`, `ui::shown_number` and `ui::shown_slider_value`
  give the value a number setting steps or slides to: for the size of
  book text the next one the reading font draws differently.
  `ui::next_value` steps so.
- `xml::Tokenizer::try_feed` tokenizes as `feed` does, but where the
  heap has no room for its buffer it returns an error rather than
  stopping the program. The buffer grows by 512 bytes at a time, never
  by doubling.
- `FontHandle::largest_size` says the largest size a font has. For the
  kernel, `Settings::set_range` and `Settings::set_description` narrow a
  setting's range and describe it.
- The SDK grew a lot: reading apps, lookup providers, voices, page
  layers, places for widgets, building blocks for screens, one choice
  and confirm dialog, declared work and power modes, typed events and
  settings, where an installed plugin came from (`install::origin`),
  installs file by file (`install::local::Staging`), serving over the
  USB cable (`served::CABLE_PORT`), typing into a text field
  (`events::TYPE_TEXT`), and book data that is made again apart from the
  reader's own (`add_book_cache`, `add_book_data`). It has breaking
  changes before 1.0; `api.txt` in `crates/paperpaw-sdk` lists its
  public API, and CI checks it.

#### Fixed

- What a plugin tells of the PaperPaw it runs in comes from the kernel
  that runs, not the SDK the plugin was built with:
  `web::user_agent_of(env)` names its version, `install::fits_in(env, ..)`
  takes its SDK (`install::fits_sdk` where no `Env` is at hand), from
  `Plugins::kernel`. The kernel's manifest now has the version of its
  crate. `web::user_agent()` and `install::fits()` stay, for the SDK a
  plugin is built with; PaperPaw's own plugins use the new ones.
- The files of a data plugin are checked by their SHA-256 where the
  plugin library gives it (`sha256` beside `sha1` in `latest.files` and
  in an update's `files`), else by their SHA-1 as before; downloads
  kept on the card with `sha1:` go on (`install::Digest::take`).

#### Devices and tools

- `paperpaw-dev` takes font packs (`.ppfp`) in a data plugin's `fonts/`,
  where the kernel reads them, and names one elsewhere. It no longer
  checks the X4 Classic by default, and `--list-profiles` says it is not
  supported yet. A release no longer lists the X4 Classic's firmware:
  it is built, but not yet tried on one.
- `paperpaw-dev` checks a plugin on the emulated X4, X4 Pro and Boox, in
  every language, light and dark. It finds cut text, uneven margins,
  small touch targets, missing translations and screens that buttons
  cannot reach, and measures heap and frame time. `paperpaw-dev flows`
  plays scripted flows, also at their edges.
- The web emulator runs PaperPaw in a browser as WebAssembly. The
  desktop emulator can be driven over `--stdio`, writes a registry of
  settings, widgets and actions (`--registry`) and checks a plugin
  (`--check`).
- The emulator's release ZIP for each computer
  (`paperpaw-emu-<version>-<target>.zip`) holds `paperpaw-sign` beside
  `paperpaw-emu` and `paperpaw-dev`, for signing plugins without a build
  of one's own.
- The web emulator's release asset (`paperpaw-web-<version>.zip`) is
  signed as the emulators are, and holds everything its pages load. Its
  frame loads no inline style or script, so it runs under a strict
  Content-Security-Policy from any path, as paperpaw.org's "Try it out"
  serves it (docs/web-emulator.md says what a server needs). The frame
  has no background of its own.
- The emulators' device profiles say where each physical key sits on
  the device's body and which computer keys press it (`body` in
  `paperpaw-emu --registry` and in the web emulator's device). The web
  emulator's frame draws the body with the keys there, for the mouse,
  touch and the keyboard, in the embedding page's colours (`look`).
- `tools/usb-transfer.py` and the emulator's `--cable` speak the USB
  transfer's lines (`docs/usb-transfer.md`); `Kernel::type_text` and the
  X4's USB remote type into a keyboard.
- Each release also brings the desktop emulator for Windows, macOS and
  Linux with `paperpaw-dev`, the web emulator and the handbook, each
  signed. `docs/porting.md` says how to add a device.
- The workspace publishes to crates.io as a pre-release
  (`tools/publish-dry-run.sh 0.5.0-beta.1`, and the *Publish check*
  workflow).
- `paperpaw-dev` warns of a settings group without an order or an icon,
  and of a widget's settings in a group of the main page rather than the
  widget's own (`widgets::settings_group`). SDK: `Kernel::menus`.
- `paperpaw-dev` finds an explanation of several lines cut short
  (`visual.explanation-cut`, an error): the SDK's text blocks now say
  what they draw, "…" included, and a text of the language with a "…" of
  its own no longer passes every "…" as one. It warns of a setting's
  description longer than its row (`lint.help-cut`). SDK:
  `ui::note_fits`, `ask::confirm(…).only_yes()`.

## [0.3.0] - 2026-10-16

- Screen lock (off by default): a PIN, a pattern or a password asked for
  when PaperPaw starts and when the device wakes up, set under System ›
  *Screen lock*. The code is kept only as a salted hash. Locked, nothing
  else can be used; after five wrong tries the next one waits half a
  minute. *Lock now* locks at once from any button, gesture or rule.
  It looks like a phone's: the time and the date above, round keys
  below, rings for the digits still to come.
- `Canvas::draw_line`.
- Plugin SDK: input filters (`Registrar::add_input_filter`) see every
  button and finger before screens and key maps do, and may take it or
  run an action instead; refresh filters (`Registrar::add_refresh_filter`)
  change how the panel is refreshed. `InputEvent` and `ButtonState` are
  part of the SDK.

- Plugin SDK: services, what one plugin offers others
  (`Registrar::add_service`, `Env::services`), and points plugins define
  for others to contribute to (`Registrar::contribute`); the event
  `core.library-changed` (`library::changed`), emitted when books come
  into the library or leave it (Wi-Fi transfer, catalogs, Calibre, the
  file manager), and the library shows them at once.
- Rules can have a condition: only between two times of day, only while
  reading or while not, only on battery or while charging, only while a
  switch is on or off (hold a rule, *Only if …*). They can also set a
  number (e.g. the light to 20) and show a message.

- Drag and drop in lists, as on phones: entries of menus (*Customise
  menus*), the widgets of the sleep screen and the items of status lines
  carry a grip (≡) at their end; drag it to move the row, a bar shows
  where it lands. The status line's chosen items now come first, in
  their order. Buttons move rows as before.
- Plugin SDK: `ui::Reorder` (rows moved by dragging a grip, for any
  `ListView`), and screens get a finger before the swipes in from the
  edges do (`Screen::on_pointer`), so grips and handles at the edges
  work.
- The quick panel has its name in *Customise menus*.

- Rules know many more moments: at a time of day on chosen days (every
  day, Monday to Friday, at weekends, one day), every so many minutes,
  when a book is closed or read to its end, when the device goes to
  sleep, when the battery falls below a percentage, when charging begins
  or ends, and when a switch is turned on or off.
- Plugin SDK: events `core.book-closed` and `core.book-finished` (the
  reader emits them), and `Screen::on_close`, called when a screen is
  closed.

- Gestures as on phones: up from the bottom edge goes home, down from the
  top edge opens the quick panel, in from a side goes back (while reading
  it turns the page). The quick panel holds night mode, the light,
  turning the screen, refreshing the screen, a screenshot, sleep and the
  settings, with the time, date and battery above (what shows there and
  how large is set under Appearance); plugins and the menu editor change
  what it holds. *Refresh the screen* (`core.refresh-screen`) clears
  e-ink ghosting with a full refresh, from the quick panel or any button
  or gesture it is bound to. Swipes in from the edges come before the screen sees them.
- More gestures as on phones: up on the home screen shows every tool at
  once (*All tools*, `home.all-tools`), down on it searches the library,
  and two fingers apart or together make the text larger or smaller
  while reading (`pinch.out`, `pinch.in`, in a section *Two fingers* of
  *Buttons and gestures*). The first home screen on a touch screen
  explains the gestures once (*Hint on the gestures* under Appearance
  shows it again).
- Gestures per tool: as standard, the tool's own (gestures go to it, a
  swipe in from an edge needs a second one: for games and drawing) or
  off. A tool sets its default (`gestures.policy` in its key map); the
  user changes it in *Buttons and gestures*.
- *Buttons and gestures* is easier to find one's way in: where a change
  applies (everywhere, home screen, reading, library, any tool), inputs
  in sections (edges, swipes, taps with a picture of the zone, held
  taps, buttons, held buttons), and the actions to choose from in groups
  with a search, *As standard* and *Nothing* first.

- The home screen is a grid, as on a phone: 4 columns by default (rows
  to fit the screen; both settings), each widget taking a rectangle of
  cells, each tool an icon in a cell of its own; empty cells stay empty.
  A long press or the pencil shows the grid: drag an item to move it, by
  its corner to size it, tap it for its options (move or size it with
  the buttons, its settings, remove it); *Add* adds widgets, tools and
  pages. The status line sits above the grid. Pages written before are
  placed in the grid by themselves.
- Widgets have more settings of their own: the clock's size (also as
  large as its cells allow), the date's size, whether the continue card
  shows its cover, its progress and a frame, and for the recently read
  books their heading, their titles, where the titles sit and how many
  books show. Tools with or without their names.

## [0.2.0] - 2026-10-16

- Books from a catalog (OPDS) or from Calibre keep the title, author,
  series and tags the source gives, where the book names none itself
  (text files, PDFs): `library::provide`.
- *Say what is chosen* (System, where the device speaks): says the row
  or button the buttons move to, with a switch's state, for using
  PaperPaw without looking at the screen.
- Screens say what they show: rows with their text and
  focus, buttons, switches and headings, told by the drawing helpers as
  they draw (`Canvas::describe`, `semantics::parts`,
  `Kernel::describe`). The emulator's scripts tap by name
  (`tap-on:Display`) and print a screen's parts (`describe`).
- The library is a service (`library` in the plugin SDK): what is known
  of books (title, author, series, tags, cover, progress, and fields
  plugins add), the books read last and the favourites, kept by the
  kernel from the reading events. The library, the home screen and the
  sleep screen show the same data instead of keeping copies of their
  own; what the library kept before is taken over.
- Bookmarks and highlights shared with KOReader (`koreader` plugin), for
  books read in both on one device or in one synced library folder:
  what KOReader has in `Book.sdr` comes over when a book is opened, and
  what is new here goes to KOReader the next time it opens the book;
  removing one, or a later note, goes across too. EPUB only.
- EPUB positions as KOReader writes them, down to the character: reading
  positions synced with KOReader land on the word, not the paragraph.
- Plugin SDK: bookmarks and highlights are kept by `annotations`, for
  every plugin, with when they were made and changed; event
  `core.annotations-changed`. `Document::xpointer` and
  `Document::locate_xpointer` are exact to the character.
- Colour pictures on colour screens (ADR 0015): covers, pictures in
  books, comics, OPDS covers and the sleep screen's pictures are shown
  in colour on phones and tablets and on colour e-ink (Boox models named
  "Color" or "C", with colours strengthened for the filter); text and
  the interface stay gray, monochrome e-ink is unchanged. JPEG (also
  progressive), PNG and GIF are decoded in colour where asked. Plugin
  SDK: `ImageSink::wants_color` and `row_rgb`, `ColorFile`,
  `ColorWriter`, `ImageFile`, `image::picture_from_file`,
  `OpenOptions::color`, `Formats::options`, `Canvas::draw_rgb_row`,
  `Framebuffer::set_saturation`, `ColorPanel::boost_percent`.
- Smaller text by default: menus 9 pt (was 11), status lines 6 pt
  (was 8). Both stay adjustable in the settings.
- Dictionary: the page of a long entry no longer covers *Online*.
- Text and paper colours on Android (Appearance): eight of each, or
  one's own colour code (e.g. #5b4636); night mode swaps them.
- Room between widgets: *Space*, a gap (small, medium or large), and
  *Free space*, which takes the room the others leave, e.g. to put the
  clock in the middle or the tools at the bottom. Both may be added more
  than once, on the home screen and the sleep screen.
- Appearance: *Corners* (square, slightly rounded, round) for
  highlighted rows, buttons and boxes, and *Contrast*: strong draws gray
  hints and notes in black, for screens with few grays. The Airy theme
  sample rounds its corners. Plugin SDK: `UiStyle` gains `corner` and
  `strong` (with `UiStyle::faint` for gray text), `ui::fill_round`,
  `ui::stroke_round`, and the canvas rounded rectangles.
- GIF pictures in books, comics and the sleep screen's own pictures: the
  first picture of the file (also of an animation, interlaced or with
  transparency), decoded to gray row by row like JPEG and PNG.
- Plugins can bring a stylesheet for books: a data plugin's
  `styles.css`, or `Registrar::add_stylesheet`, applied after a book's
  own and before one's own (EPUB). SDK: `Env::stylesheets`,
  `OpenOptions::css` (no longer `Copy`).
- Books' embedded fonts in WOFF are used too (WOFF 2 is not read).
- Calibre's wireless device connection (`calibre` plugin, in the tools):
  the device finds Calibre in the network (or connects to the address
  set), and Calibre sends books to the library, lists and deletes them
  as with a reader on a cable; a password set in Calibre is asked for
  once. SDK: `sha1`, and `json::raw_member`, `json::unquote`,
  `json::string_array`.
- Sockets for plugins (ADR 0014): TCP connections that read text or
  files of any size into storage, and UDP broadcasts to find services,
  in the emulator and on Android (`Command::Connect`, `SocketSend`,
  `SocketRead`, `Disconnect`, `Broadcast`).
- Small capitals (CSS `font-variant: small-caps`, `font-variant-caps`)
  in EPUB, HTML and the other converted formats: lower case letters as
  smaller capitals, the text itself unchanged (search, selection and
  notes as before). Books are prepared anew once. Document model:
  `TextStyle::SMALL_CAPS`; embedded font families at most 31 (was 63).
- Snapshot tests: screens of the X4 and the Boox are drawn headless and
  compared with reference pictures, so layout faults fail a test.
- The folder chooser says when it is still opening a folder (slow SD
  cards) and ignores taps until it shows it.
- Widgets have settings of their own, from the home screen's editor:
  where the status line, date, clock, today's reading and the word of
  the day sit (left, centre, right), and whether the clock shows the
  date. Plugins give their widgets settings the same way
  (`widgets::settings_group`, `widgets::align_setting`).

## [0.1.1] - 2026-10-15

The Android app for F-Droid.

- The app is built reproducibly: with the same tools (shells/android/
  versions.env) it comes out byte for byte the same anywhere, so F-Droid
  can publish it signed with PaperPaw's key. A dependency's macro that
  wrote its code in a random order is patched (patches/jni-macros).
- The app's library is stored in the APK and runs from there, without
  symbols: 6.8 MB to download, less space taken on the device.
- The licence names the copyright holder; README with screenshots.

## [0.1.0] - 2026-10-15

The first release: PaperPaw on Android e-readers (tested on a Boox Go 7)
and in the desktop emulator, with its plugin SDK. The firmware for the
Xteink X4 is built but has not run on a device yet.

### Reading

- EPUB 2 and 3 (with the book's styles, embedded fonts and pictures, or
  one's own stylesheets per folder or book), FB2, Markdown, plain text,
  comic archives (CBZ), Xteink's XTC and XTCH, and PDF on Android.
- Contents, search, bookmarks and highlights with notes, export of notes
  as Markdown, a dictionary (StarDict), Wikipedia and translation
  (DeepL), a vocabulary list of words looked up.
- Hyphenation (German, English), reading fonts with weight and size,
  margins, spacing, status lines to taste, reading aloud, pages turned
  by themselves, turning the screen.
- Reading statistics, shared with KOReader (also over WebDAV), and
  reading positions synced with KOReader's sync server.

### Books and more

- A home screen of widgets on pages (date, battery, continue reading,
  recently read, tools, statistics, headlines), editable by touch or
  buttons.
- The library by folders, authors, series and tags; a file manager;
  books from OPDS catalogs and WebDAV folders (Nextcloud), kept in sync.
- News feeds as books; Wallabag's read-later articles, highlights to
  Readwise and a browser, switched off until wanted; transfers from a
  computer or phone over Wi-Fi, typing on a phone.
- A sleep screen (cover, picture or widgets), screenshots.

### Making it one's own

- Every screen, menu, key and gesture can be changed, in the app or in
  `paperpaw.ini`; profiles, rules, themes, translations and fonts as
  plugins without code (data plugins).
- Settings with descriptions and a search, passwords and keys in one
  place, short menus by default, usable by touch and by buttons alike.
- English and German.

### For developers

- The plugin SDK (`paperpaw-sdk`) with a guide ([Writing
  plugins](docs/plugins.md)) and a sample plugin; plugins are tested
  against the real kernel.
- The desktop emulator with device profiles and memory budgets.
- The Xteink X4's firmware, its display, buttons, battery and SD card
  tested against simulated hardware ([ADR 0013](docs/adr/0013-x4-firmware.md)).

[0.3.0]: https://github.com/hthoene/paperpaw/releases/tag/v0.3.0
[0.2.0]: https://github.com/hthoene/paperpaw/releases/tag/v0.2.0
[0.1.1]: https://github.com/hthoene/paperpaw/releases/tag/v0.1.1
[0.1.0]: https://github.com/hthoene/paperpaw/releases/tag/v0.1.0
