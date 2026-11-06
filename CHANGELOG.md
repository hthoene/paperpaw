# Changelog

What changes from one release of PaperPaw to the next. Versions follow
[Semantic Versioning](https://semver.org); before 1.0, a minor version may
change the plugin SDK.

## [Unreleased]

### Added

- Interface-7 SDK callback host binding and `native_plugin!` entry glue, with checked table admission
  and fallible host allocation. Storage slots 20-26 use caller-owned buffers; event tags 32-37 decode
  into borrowed payloads. These facades do not activate any shell loader (#228).

- Host-side storage adapter for interface-7 slots 20–26 (path rules, free-space reserve, per-plugin
  read/write/sync counting), not wired (#228).
- Interface-7 draft: host-table slots 20–26 (`storage.size`, `read_at`, `write`, `append`, `rename`, `remove`,
  `sync`) declared in `abi/abi.def`; the default table answers `ERR_UNSUPPORTED` until they are wired. The
  storage contract records owner decisions 124 (uninstall asks before deleting the data folder) and 125 (root 1
  for the user's files, gated by a signed `uses.storage`) (#228).
- Interface-7 draft: service records 48-52 (`service.usage`, `plugin_usage`, `plugin`, `work_note`, `end`) and the
  four data services of slot 17 in the metadata catalog (semantic revision 0, no call table), with a generated
  codec for one bare record (`record_framed_len`, `record_encode_one`, `record_decode_one`). A plugin cannot submit
  them (#228).
- Desktop emulator: a developer switch, `--abi7-dev-loader` or `PAPERPAW_ABI7_DEV_LOADER=1` (Linux x86-64, off by
  default), loads interface-7 plugin libraries (`plugins/<id>/code-host.so`, told from their bytes) for loader work. What a
  plugin declares is staged but not applied yet, and nothing is drawn; interface-6 plugins are unchanged (#228, #772).
- Interface-7 draft: statuses `ERR_NOT_FOUND` (-10) and `ERR_FULL` (-11), and record codecs generated from
  the fields declared in `abi/abi.def` (optional appended fields, tag ranges per record family) (#228).
- Interface-7 draft: event records 32-37 (`started`, `resume`, `suspend`, `stopping`, `battery_measured`,
  `page_turned`) with host encoders in `native_host::events`; a plugin can register for them (#228).
  An optional scalar field declared with `"default": null` is generated as `Option<T>` (present exactly
  when its bit is set): `page_turned`'s `page` and `pages`, which have no value meaning "not counted".
- `tools/hyphenation-packs`: builds and checks the thirteen hyphenation plugins as data plugins
  (`[hyphenation]` file kind, PPHP version 1 tables); the interface-6 plugins stay until the
  card-backed reader lands (#228).
- `paperpaw_sdk::native::declare` (interface 7, merges with the raise): a builder for the declaration records 5-10 (setting groups, specs and extras, hyphenation, font faces and extensions) that splits them into batches and encodes them with the generated codec, with golden settings batches of the widgets and Battery plugins as fixtures (#228).
- `paperpaw-hyphen`: `check_table`, `Layout`, `TableError` and the limits `MAX_BLOCKS` (4096), `MAX_BLOCK_BYTES` (512) and
  `MAX_EXCEPTION_BYTES` (8 KiB) behind the `kernel` feature (not seen by plugins), the one check of a pattern table that the
  kernel's reader and `paperpaw-dev package` share (#228).
- Interface-7 groundwork: generated typed settings values and scoped settings/translation
  copy-out adapters, with bounded handle and callback admission checks (#228).
  The new host table is not yet activated.
- Host-only pinned plugin owners and transactional registration staging, with
  explicit cleanup ownership on commit failure (#228). Runtime wiring remains inactive.
- Scoped submission of validated host records during plugin construction (#228),
  clearing access on return or unwind and latching validation refusals even when
  ignored by the caller; foreign registration decoding remains unwired.
- Generated optional event/draw callback tails, bounded table readers and callback
  minor requirements for the interface-7 prototype (#228). No live dispatch yet.
- `native::DRAFT`: the draft of the plugin interface, part of `native::FINGERPRINT`, so a plugin built for
  an earlier draft is refused. Freezing the interface (the owner's decision) deletes it (#228).

### Changed

- **Plugin interface 7, draft 1** (#228, early raise on `abi7/integration`): `native::INTERFACE` is 7 and
  every plugin with code needs a build for it; plugins built for interface 6 are refused as built for an
  earlier interface. Until it is frozen, a change plugins rely on takes the next draft, named in
  `abi/interface-7/blessings.txt`; CI (`tools/abi-reference-check.py`, replacing `abi-frozen6-check.py`)
  refuses an unnamed one against the PR's base. The temporary partition of the interface-7 host table
  from the frozen interface-6 files is retired.
- `DATA_FORMATS` is 5: data plugins may name hyphenation tables in `[hyphenation]` (`hyphenation/*.pphp`),
  so the 13 hyphenation plugins install.
- **Plugin interface 7, draft 2** (#228, `abi/interface-7/raise.md` row 2): hyphenation from data plugins is
  used on a page. `Hyphenation::get` returns a `Hyphenator` (`with_minimums`, `hyphenate`) instead of
  `&Patterns`: a table compiled into a plugin, or a data plugin's table read from the card a few blocks at a
  time, which comes first. At most two card tables are open, within 48 KiB of heap together, checked
  before anything is taken (about 20 KB for `de`, the largest); only a table idle for about a page of words
  is closed for another, so two languages in use never close each other at every word. The kernel opens
  them (an opener it sets with the storage), so plugins built without the `kernel` feature hyphenate with
  them too, and closes them when a book is closed, before sleep and before Wi-Fi, network, store-install
  and firmware work (`Hyphenation::close_tables`). A read that fails closes the table and the next word
  opens it once more, a second failure gives the language up until the card is mounted anew. `PageType::patterns` returns the `Hyphenator`. The kernel keeps the
  data plugins' tables in the SDK's `Hyphenation` (the greatest plugin id has a language).
  `paperpaw-hyphen` (kernel): `StoredPatterns::open_within` refuses a table over a heap budget
  (`OpenError::OverBudget`), `StoredPatterns::heap_bound` bounds it from its header.

## [0.6.0-beta.3] - 2026-11-05

### Added

- **The Wi-Fi page: the networks have a page of their own** (#161). The Wi-Fi page is short whatever is in the air:
  the switch with where it is, a row *Networks ›* (the network the device is in and how many are in reach; it opens
  the list), the setup with a phone and the three settings. While no network is kept, *Set up with a phone* comes
  first, below the switch. Back on the list returns to the Wi-Fi page; if the radio goes off meanwhile it does
  too. Opening the list does not scan again (the Wi-Fi page has just); its last row looks again. Both pages measure
  their rows again only when what they say changes, and an event of the radio that changes nothing they show (a
  signal that moved a little) draws nothing: each frame costs the X4 a screen refresh, and a key pressed meanwhile
  waits for it (#160, partial: the cause of the slow Up and Down is not settled).

- **Spanish user interface.** Kernel and plugin menus in Spanish, as the data plugin `language-es` from the store.
  Machine-translated and checked with `tools/language-pack.py check es`; corrections from Spanish readers are welcome.
  New or untranslated messages fall back to English. Spanish dates, shorter labels and consistent *Elegir* are
  included (#170).
- **Colours of widgets** (#206, SDK). Each widget has a paper (`<widget>.paper`: normal, inverted, light gray, dark
  gray, transparent; on colour panels also any tone or a code such as `#3b6fd4`) and a text colour (`<widget>.ink`,
  automatic: black or white, whichever reads on the paper), and the kernel's `core.widgets.paper` and
  `core.widgets.ink` set them for every widget that has none of its own. `widgets::widget_look` gives the colours and
  `widgets::draw_frame` paints the paper, so widgets that draw in `look.ink` on `look.paper` follow without a change;
  `widgets::colour_spec`, `paper`, `ink` and the settings' builders are new. `GridView::draw_on` takes what lies
  under the items (`widgets::Plate`: white as before, nothing over a picture, a colour), so widgets can sit on a
  picture or a dark page. Additive: plugin interface 6 stays, plugins built before draw as they did; their widgets
  follow the colours, those of all widgets too, once rebuilt with this SDK and run on firmware with the shared
  widget family described below. Transparent widgets take the page to be light: on a dark picture, choose a light text colour.
- **Shape of widgets** (#206, SDK). Each widget has corners (`<widget>.corners`: automatic, square, slightly
  rounded, round, soft), a frame line (`<widget>.frame-line`: automatic, thin, medium, thick) and a solid shadow
  below and to the right of its card or paper (`<widget>.drop-shadow`; dark gray below a black plate, which a black
  one would not stand out from, dithered where the panel has few grays). The kernel's `core.widgets.corners`,
  `.frame-line`, `.shadow`, `.padding` and `.gap` set them for every widget that has none of its own; "Widget look"
  presets cover them. `widgets::draw_frame` draws them, so widgets need no change; a widget that draws all of itself
  stays as it is. `widgets::widget_look`, `draw_frame`, `frame_height` and `draw_caption` now come from the
  firmware (shared-table family `widgets`, `Family::WIDGETS` = 6, revision 1; `Family::CORE` is deprecated), so
  each widget plugin is 3.5 to 5.8 KB smaller on the X4 and its widgets follow the firmware's look. Plugins built
  with this SDK that have widgets need this firmware: beta.2 refuses them cleanly ("needs a newer PaperPaw") and
  the store does not offer them to it; plugins without widgets are unaffected. Plugin interface 6 stays.
- **Picture home screen** (#205), a store plugin (`picture-home`, group Look). Your own
  picture behind the widgets, from a folder on the card: fill, whole picture, stretch, original size or tile, in one
  of nine places, zoomed and made paler; prepared once into a screen-sized gray file and drawn from it, a few rows at
  a time. Pages of widgets and tools placed anywhere in the SDK's grid, on plates over the picture. The status bar is
  yours to configure: on, off or automatic (a button press or a tap shows it for a few seconds, and that press does
  nothing else), top or bottom, full width or a compact pill at the left, middle or right, on a light, inverted, light
  grey or dark grey plate or on the picture (black or white text, whichever reads). The row naming the buttons (X4)
  can be on, off or automatic. While the plugin is on it is the home screen; switched off, the built-in home screen
  is back with its own pages. Brightness and contrast controls adjust the background while keeping black and white;
  changing them prepares the picture once again.
- **Colour rows in every first-party widget** (#206): each widget of the widget pack, the home screen, statistics,
  weather, reading goal, mail, news, notes vault and vocabulary has *Colours* (its paper) and *Text* beside its frame
  and spacing. The picture home screen's *Widgets sit on* sets the plate under the widgets: white, light gray or none.
- **Shape rows in every first-party widget** (#206): the same widgets have *Corners*, *Frame line* and *Shadow* beside
  their colours, and the home screen and the picture home screen place their widgets with the gap the reader set for
  all widgets (small, normal, large).

### Changed

- **Release page: plugin files out of the firmware release** (#89, #171). A firmware release now holds 73 files; every plugin
  file and every plugin or data folder's ZIP goes to one store release of the year (`plugins-2026`, a pre-release that is
  added to and never replaced; only versions not published before; a numbered successor when the projected count exceeds
  800 assets). The release uploads them before it creates the firmware release, so the catalogue never names a file
  that is not there. The workflow
  *Store plugin update* now takes several plugins at once, and builds from `main` against the base release's fingerprint;
  both workflows have a `dry_run`. Releases already out are untouched.

- **"More plugins" is now "Plugin store"** (#213; de: "Plugin-Store"). The name of the store on the device, its title
  and the help texts that send you to it.

### Fixed

- **Dual boot: no false positive on plain devices, and a way out** (#224). The dualbooter's `x4boot` tag counts only
  as a word of its own (not inside `x4bootloader` or a path); the bootloaders PaperPaw ships for X4, X3, X4 Pro and
  X4 Classic, CrossPoint's and others are scanned in a test and none is taken for a dual-boot one. Where a device is
  taken for one wrongly, the notice on the Updates page has a button, *This is not a dual-boot setup* (de: *Kein
  Dual-Boot-Gerät*): after a question that warns an update can overwrite a second firmware, it writes `dual-boot =
  off` in `paperpaw.ini` and restarts, and updates work again. Where the file says off, *Detect dual boot again*
  writes `auto`. Update plugin 1.0.6.
- **Dual boot: no write to the OTA data before the setup is known** (#222). Where the bootloader carries a
  dual-boot tag (`x4boot` or `paperpaw-dualboot:1`), an update PaperPaw put on trial itself, or one that failed its
  trial, is no longer marked (or gone back from) before the card is read: nothing is written to the OTA data at all,
  so the dualbooter's choice of slot stays. A setup declared only by `dual-boot = on` in `paperpaw.ini` cannot be
  known before the card is read; there a trial PaperPaw itself began is still marked at once, as before.
- **X4 beside the X4 dualbooter** (#167). The X4 finds the slot it runs from by its own code, so a dualbooter's
  bootloader no longer hides the OTA state. PaperPaw recognises the dualbooter's bootloader by its `x4boot` tag
  (besides `paperpaw-dualboot:1` and `dual-boot = on` in `paperpaw.ini`), decides at every start, writes
  `.paperpaw/boot.txt`, and the Updates page says that the device holds two firmwares and installs nothing
  here, also where it has no firmware information. Update plugin 1.0.5.

- **Card language packs keep translating later commands** (#208). The kernel reclaims a nearly full message
  cache between commands, so a preceding callback cannot exhaust translations for later notices and dialog labels.
  Smaller caches stay resident; the 16 KiB budget and plugin interface stay unchanged.

- **Arranging the home screen no longer turns the page or opens the tools by mistake** (#215). While the page is arranged, a
  swipe or tap that hits nothing (a finger straying off a widget) does nothing, instead of turning the page or opening
  the menu. *Done* and *Add* are buttons at least two rows wide, with the word *Done*, instead of a small tick that taps
  missed and that landed on the page.

- **The store's lists are not left empty when the list arrives** (#169). A group (e.g. Languages) opened while a fresh list of
  plugins was still being fetched showed its pages with every row blank, only the lines between them: the fresh list took
  the place of the one the open list had picked its plugins from. The open list now picks again from the fresh one.
- A rotated picture-home photo is prepared only for the part the screen shows, avoiding a full-image transpose
  at high zoom (#205).

- Adding a UI language pack no longer changes the shared language builder or makes CI ask for version
  bumps of other languages (#172). Language metadata comes from its catalog header, and store texts and
  dependencies from its data-plugin entry. Existing language packs build to the same bytes.

- **The plugin manager reads only the rows it shows** (#208). It translated the description of every plugin to find how high the
  rows must be, which filled the 16 KiB a language on the card may read in one go; later texts of that call came out in
  English. Now only the rows of the page shown are read, and the rows get higher, never lower, when a page needs it.
- **The store shows every plugin after an update, without a restart** (#164). The first start after an update runs on trial
  and loads no code plugins, on purpose, and the kernel then did not know what the firmware takes: the store asked for no
  builds and showed only data plugins (Look, Languages) until the next cold start, which a Pro on light sleep may not have
  for days. Nothing changes during the trial (no code plugin is offered or installed, so a bad update still goes back and
  the plugins before stay as they were), but once the update is kept, its mark written for good (about 30 seconds on), the
  kernel tells plugins what the firmware takes: the store, opened or opened again, lists and installs the builds that fit
  (*Restart now* as for any code plugin); a mark that could not be written keeps it as before.
- **Widget plates follow rounded corners** (#206). A grid plate no longer sticks out past a widget's corners or
  fills the space beside its shadow. Frameless paper follows explicitly chosen corners; Automatic stays square.
  Empty widgets have no grid plate outside editing mode.
- **X4 Pro: the charging bolt on battery, and a sleep that never switched off** (#209). The Pro took "a computer sends USB
  frames" for charging: the bolt, the kernel's low-battery rules, its install check and the sleeping reader's *Switch off
  after sleeping* all went by it, so a Pro on battery could show the bolt, run no idle naps and stay asleep all night.
  Charging is now the charging pin alone (the X3: its gauge and the frames, as before); the frames only keep an awake
  reader from napping while a computer is on the cable, and count only when the frame number moves on like a bus. The
  Pro's charging pin is high on a cable and low on battery (measured on a Pro; the first port had it the other way round). A reader asleep on a cable with a full cell is switched off after its time like one on a
  battery; while it charges it stays asleep, which the setting's help text now says (en, de). The diagnostics page
  shows the charging pin's level and whether USB frames come on the Pro and the X3 (diagnostics 1.0.7). The X4 is as before.
- **The empty start card reads on a dark card** (#207). Its words were black on the black card of a dark frame (and on
  an inverted paper); the home screen draws them in the widget's own ink now, as its status line does. The pack's
  world clock, library numbers and weather forecast chose their lines and shades by the frame being dark; they follow
  the paper's lightness now, so a dark gray or inverted paper works too.
- **A book opened before its font pack came no longer keeps a wrong page count** (#163). A Hebrew book opened before the
  Hebrew letters were installed was counted with those letters measured as `?`: 2 pages for a book of one. The count was
  kept under a name that did not change when the pack arrived, so the book showed `1 / 2` and no turn went to the
  second page. The pages' key now covers the fonts and packs there are (a pack that comes or goes makes the pages be
  counted again, also while the book is open), and a turn that finds no page though the count says more throws the
  count away and makes it again. Reader 1.0.8.
- **The button bar speaks a store language from the first frame** (#204). A screen that read much of a language pack as
  it opened (the plugins list reads every plugin's name and description) filled what is read of the pack at once, so
  the bar's labels fell back to English (*Back*, *Select*) until the next redraw. The kernel now forgets the messages
  read before it looks up the bar's labels when they near that limit, as it already did at the end of each call.

## [0.6.0-beta.2] - 2026-11-03

**Plugins built for 0.6.0-beta.1 keep working; rebuild only to use the new items.** (Plugin interface 6 is unchanged; the SDK changes only additively: `SCREEN_REVISION` 3, `events::MENU_PAGE`, `Cx::is_on_top`, panes, `ScreenClass`, `StorageError::CARD_GONE`, `ui::SearchField`.)

### In plain words

This is a beta. Keep a copy of your books. The details for plugin authors follow in the sections below.

- **Plugins built for 0.6.0-beta.1 keep working** (plugin interface 6 stays). Rebuild a plugin only to use the new items in this release.
- **The first update from 0.6.0-beta.1 has no early rollback marker.** A bootloader with rollback and the web flasher still recover a device that hangs on that first start; later updates can be rolled back early.
- **PaperPaw beside another firmware** (#95). A tool that puts two firmwares on one X4 can tell PaperPaw so (`dual-boot = on` in `paperpaw.ini`, or a marker in the tool's bootloader). PaperPaw then never writes the other partition or the update data. Not declared: nothing changes.
- **A memory card taken out and put back** (#145). The library no longer says "Preparing the library …" for ever: it says at once that the folder cannot be read, shows the books again when the card is back, and asks for a restart where the X4 had to give the card up. No first-run hints over a card that cannot be read.
- **X4 Pro: a real sleep before it switches off** (#153), and a shorter wait to wake on the X4 and X3 (the light sleep's watchdog counts in the sleep).
- **Switch off leaves a picture** (#146): *Switch off* in the power menu shows the sleep picture first, so the device no longer looks frozen.
- **One search field everywhere** (#113): the library, settings, the store and the button settings use the same field.
- **The first walk of a big library reads no covers** (#398), **a list beside its detail on wide screens** (#88) and **the reader's menu shows the page and the pages** (#117).
- **Less waiting** (#118): the busy mark shows less often, and settings screens open faster.
- **The store** shows all the plugins of this PaperPaw, lists a *Languages* group (#148), installs the plugins a plugin requires (#97) and shows the download for a book that needs more letters (#147). The update check says how long the device is in use (#143) and tells when some plugins did not come along (#141).
- **Stay awake on USB** (#96): a developer setting, off by default, so a computer can keep the link for remote tests.
- **The device check** steers its first pages with the buttons (#121). The Screen page may still react slowly to the arrow keys on the X4.

### Added

- Storage: `StorageError::CARD_GONE` (code 1 of `StorageError::Other`, #145): the memory card was taken out and
  cannot be read again in this run (put back more often than a run allows, or holding nothing PaperPaw reads);
  nothing is read or written until PaperPaw starts again. The X4 firmware's storage says it once it gives a card up,
  and the library then asks for a restart (*Restart PaperPaw*) instead of saying it tries again by itself. New
  constant, no change of the interface.
- Screens: `Screen::times_under_overlays` (`SCREEN_REVISION` 3, #117). A screen that says so has its time
  (`Cx::wake_at`) while overlays alone cover it (a menu bar, a panel), as it still shows below them: for work that goes
  on there, as the reader counting its pages. No by default: the time waits until it is on top again, as before.
  Called through `call_since`, so screens built before are asked nothing. New item at the end of a growing trait, no
  change of what plugins built before see.
- Events: `events::MENU_PAGE` (`MenuPage { menu, page }`, #117): whoever opened the menu slot as a bar with a page
  text tells the open bar its new text; the menu plugin's bar shows it. New constant and type.
- **A list and the item chosen in it beside each other on wide screens** (#88). Where the screen is expanded (a
  tablet, a phone on its side; never the X4, which is compact in both orientations), a screen that says
  `Pane::DETAIL` shows beside the full screen below it: the list at the start (on the right in a right-to-left
  interface), at least 60 mm or 38 % wide, a line between them, bars and the row of hints over both, each laid out
  for its part through `Screen::on_resize`. A tap goes to the part it lands in and keys follow it; the new action
  `core.pane.other` moves them to the other part. A detail the list opens takes the place of the one shown, so
  going through a list piles nothing up; one the detail opens shows alone over both, and *Back* brings the two back.
  Free additions, no change of the interface: `ui::ScreenClass` (compact below `ui::COMPACT_BELOW_MM`, 100 mm,
  expanded from `ui::EXPANDED_FROM_MM`, 140 mm, by the width scaled by the reader's interface text size against
  its default), `Env::screen_class` and `Env::landscape` (of the screen's own part), `events::DISPLAY_CHANGED`
  (`DisplayChanged`: the display's width, height, class and whether it is on its side, sent once after a resize
  laid the screens out), `actions::PANE_OTHER` and `Command::extended`, which plugins had no way to make. Screens
  are asked `pane` once, when made. A list kept by `on_resize` while a turn puts its detail over it is told
  `on_hide`. The kernel's new `panes` feature (on by default) builds it; the Xteink firmware (X4, X4 Pro, X4
  Classic, X3), never wide, leaves it out and saves its code. There a detail is always a full screen and
  `core.pane.other` is not offered; the SDK is the same either way. Shells with the app's default features
  (`paperpaw-app/panes`) have it. A finger on the line between the parts goes to the nearer one; a change of the
  interface text size that makes the screen expanded or no longer so lays the two out again. A class a later
  kernel adds reads as `Expanded` in `DisplayChanged`, so an older plugin still hears the event.
- Devices (#95): PaperPaw beside another firmware (a tool that puts two firmwares on one X4) can be told so, by
  `dual-boot = on` in a `[firmware]` section of `paperpaw.ini` or by a marker in the tool's bootloader
  (`paperpaw-dualboot:1`, see `docs/dual-boot.md`). It then never writes the other partition or the OTA data: no
  updates, no trial, no way back to another firmware; the *Updates* page says why. Not declared, nothing changes. A
  `[firmware]` section no longer warns as an unknown plugin, and `firmware` can no longer be a plugin's id (as
  `plugins`, `fonts`, `menus` and `slots` cannot). An update PaperPaw installed itself is still marked as on trial at once, as before; what a tool put in the other
  slot, and the undoing of an update, wait until the card and `paperpaw.ini` are read. The bootloader's 32 KB and the ini are read only where a start would write the OTA data, after a declared start, or before an install; a normal start reads neither. No change for plugins.
- **One search field everywhere** (#113). The SDK draws a search field as one part, `ui::SearchField`: a rounded
  outline round the search icon and a gray hint (or the words searched for), the focus over it when focused, the
  icon at the right where the interface runs right to left, in the theme's inks. The library's *Find* screen, the
  settings, the store and the actions list of the button settings use it, so their searches look the same; the
  settings' search is now a field at the top of the list, no longer a row like the others. For plugin authors:
  `SearchField::new(hint).text(words).focused(focused).draw(ui, rect)`, `SearchField::height(style)`; a new type,
  so plugins built before are not affected.
- Devices (#96): a developer setting *Stay awake on USB* (X4, X4 Pro, X4 Classic, X3; off by default). While the cable is in,
  the sleep after *Sleep after* does not come, and *Sleep* shows the sleep picture but does not sleep, so the computer
  keeps the link for remote tests; the next press or command wakes it, and a cable pulled meanwhile puts the device
  to sleep (two seconds after, so a wiggled plug does not). On the X4 the cable is read at its pin; on the X4 Pro, X4
  Classic and X3 "on USB" means a computer talking to it or charging, so on a wall charger with a full battery the
  setting does not hold. No change for plugins.

### Changed

- **X4 Pro: a real sleep before it switches off** (#153). Sleep (the power button, the power menu, the idle time) used to
  switch the Pro off at once, as the X4 did before 0.5. Now the Pro sleeps lightly as the X4 does: the screen stays, a press of
  the power button wakes it at once, and it switches off after the time of the new setting *Switch off after sleeping* (30
  minutes by default; *At once* gives the old behaviour; *Never* stays asleep). The front light is off while it sleeps and comes
  back as set when it wakes. Every 10 minutes it wakes for a moment to look at the battery (3 % or less: off at once). The
  X4 Classic keeps its deep sleep. Not measured on a device yet.
- **The update check says how long the device is in use** (#143): next to what started the check, it sends `age`,
  `new` (under seven days since the first check), `week` (up to thirty days) or `month`, so that the store's
  statistics can count only devices in use for a while. The device keeps the date of its first check on the card
  (`first-check.txt`, never sent); without a clock set, no age is sent.
- **Settings screens skip a pass that counted pictures** (#118). On a black-and-white panel every whole-screen frame
  drew the screen once more, aside, to count its pictures; the settings screens have none, so they now say so
  (`Screen::pictures_in_grays`) and the pass is gone (about 4% of the work of opening a group and going back, on the
  host). Not the whole lag: the first open of a group in a session still shows the busy mark.
- **Opening a settings group does less work** (#118). The page asked the settings about every registered setting several times
  (making the setting's group each time); it now asks once. The key handler of Select falls by about a third on the host.
- The plugin interface's gate (ADR 0026, #94): `interface.txt` keeps the fingerprint the bless was made with, and
  a bless of a change plugins rely on is refused unless the interface rises above the one that fingerprint was made
  at, whatever number `interface.txt` says; layouts of what crosses that changed while the shapes did not are such a
  change. The first bless of the interface-6 round kept interface 5 that way and its fingerprint with it (#363). The
  fingerprint is as it was (X4: `00063e32940609b9`).
- The first walk of new books reads no covers (#398): a book's cover is read from it when a view first shows it (the
  library's list and grid, its book sheet, the home screen's books, the sleep screen), one book at a time, the books
  shown first, none begun while keys come (a pause of 0.6 s after the last); a book never shown never costs its cover.
  Until then the book shows as one without a cover does (title and author). Plugins see such a book not yet indexed
  (`Book::indexed` off, its title and author there) and read its cover with `library::index`, as for any book not
  indexed. On the bench's card the first walk of 1,000 books that all have covers writes 26,952 blocks instead of
  139,557 and reads 75,974 instead of 152,975, its peak 26.2 KiB instead of 70.0 KiB.
- **Less waiting for the busy mark** (#118). Going back no longer shows the busy mark the first time (the screen
  below is there already); what was timed is kept across starts in `.paperpaw/core/timing.txt` (written when the
  device sleeps or stops, only where it changed what is slow), so on the X4, where every wake is a start, a screen
  that opened quickly before is not marked again; and the mark shows only for work that took as long as the
  refresh showing it (500 ms, `KernelBuilder::busy_mark_after`, 400 before). A damaged or older file holds no
  times. On the X4 a needless mark cost ~330 ms.
- **The store installs what a plugin requires** (#97): after the store installs a plugin whose `plugin.ini` names
  `requires` (the Arabic, Persian and Urdu menus need their letters, Korean extra needs Korean), the plugins the
  device lacks are offered in the store's dialog with their sizes (*Download* or *Not now*). The plugin stays
  installed either way. A chain asks step by step; what is on the device or not in the library is passed over.

### Fixed

- **X4, X3: the 15-minute watchdog of a light sleep counts in the sleep** (#153). The reset that ends a sleep which never
  ends was paused in light sleep after the first sleep (the library's enable call set the pause again). It is now cleared
  after every arming.
- **The reader's menu shows the page and the pages** (#117). Opened right after a book opens, before its pages were
  counted, the menu bar showed a percentage ("0 %") for as long as it was open, as the count waited for it to close.
  The count now goes on below the menu, and the bar shows "2 / 185" once it ends.
- Tap zones (thirds, corners, the zones of key maps) are measured on the part of the screen the screen has, as
  the finger's point always was: on touch devices with the row of button hints (the X4 Pro, the emulator) the
  borders of the lower zones lay about 3 mm off, by part of the row's height (#88).
- A damaged folder on the memory card (entries with names no file system gives, as a reset can leave) can be removed
  again, and the folders above it (#388, #99): the folder is cut off where its parent names it, and nothing in it is
  written, followed or freed, the entries that look sane included (its space may hold another file's data; the space
  comes back when the card is checked on a computer). Before, removing the folder, or a book's cache in it, failed
  with "invalid path". Folders of names PaperPaw does not take (made elsewhere: `con`) are removed with all they hold.
- A book whose cache folder cannot be cleared (a damaged folder, #99) is never read from: the book shows its details,
  and opening it says the card could not be written, instead of reading another book's chapters as its own.
- **The library no longer says "Preparing the library …" for ever** (#145). If the kernel makes no catalog or
  sorted books for 90 seconds (a card taken out and put back while PaperPaw ran, sorted books that cannot be
  read), the library says that the folder cannot be read, and asks the kernel once more. Files plugin 1.0.5.
- **A memory card taken out with the library open is said at once** (#145). Turning a page then said "Preparing the
  library …" until the card was back: the kernel makes nothing of a card it cannot read, so no word of it came. The
  library now says that the folder cannot be read as soon as it finds the card cannot be read (*Restart PaperPaw*
  where the X4 gave the card up for the run), and shows the books again once the card is back. Files plugin 1.0.6.
- **No first-run hints over a card that cannot be read** (#145). With the card taken out, the hints counted as never
  shown and opened over the library as on a new card; they now count as shown where what was shown cannot be read.
  Only a card that has no record of them shows them. Hints plugin 1.0.3.
- **Device check, first page** (#121): the Screen page opened with the focus on *Pass*, the answer it suggests without measuring anything, so the first arrow went round to *Flip pattern* and a first Select passed the page unseen. It now opens on *Flip pattern*, the first tool, as pages with tools should, and *Pass* stays the answer shown filled. Also, a key on that page took seconds, or showed nothing: the page counted as best in grays, which on the X4 makes every change, a moved focus too, a refresh of all the screen in its four grays, and after it a small change is not shown at all until a bigger one comes. The page now shows its stripes in grays when it opens and after *Full refresh*; moving the focus or flipping the pattern refreshes only what changed, in black and white.
- **Device check, Buttons page** (#121): the page took every button from the moment it opened, so on a device of buttons alone the arrows, Select and Back did nothing and there was no way to move on or leave. It now opens with *Start* like the other pages' controls: the arrows, Select and Back work as everywhere until *Start* is chosen; then every button counts as the one asked for, as before.
- Setup: the 'Look for updates' step shows its label next to the switch again (#132).
- Updates: when some plugins did not come along with an update, one short notice says so (not one for each), and they are fetched again at the next update checks (#141).
- **"Switch off" in the power menu leaves a picture on the screen** (#146). It switched off with the last page still showing, so the
  device looked frozen. Switching off and sleeping from the power menu now show the sleep screen first, as the power
  button's short press does. Where the sleep screen is set to show the page as it is, or the page with a picture, switching
  off shows the logo with "Switched off" and how to switch on instead (English and German).
- **The plugin store shows all the plugins of this PaperPaw** (#148). It now tells the plugin library which PaperPaw runs
  (`abi`, the fingerprint) in its list and page requests; without it the library answered with the builds of PaperPaw
  0.5, which this one drops, so only a few code plugins were listed. A plugin built for an older interface than the
  device runs is offered as an update where the library has a build for this one, whatever the version says; one the
  library has no build of yet is listed as "Not available yet", not left out. Store 1.0.5.
- **Language packs have a group of their own** (#148): *Languages* (*Sprachen*), before *System*. Readers that do not
  know a group show it under *System*; a group this store does not know can no longer cause an error.
- **A book that needs more letters shows the download** (#147). After *Download* a screen shows what is fetched, a bar
  with the percent and the bytes so far, and how it ends: it closes and says *Ready* (the book shows its letters, no
  restart: the packs of letters are data plugins that work at once), or it says what did not come (no connection, no
  room) with *Try again* and *Close*, or *Restart now* / *Later* for a pack that needs one. Leaving the screen lets the
  download go on. At the largest text the hint keeps all its lines and the percent stays clear of it. Store 1.0.5 (the same as #148).

## [0.6.0-beta.1] - 2026-11-03

**Plugins of this release must be built from `965d866b` or later** (plugin interface 6, #363; `965d866b` raises the shared interface family `ui` to revision 2, #119: the firmware refuses plugins of revision 1, built before). 0.6.0-beta.1 takes
the interface-6 round too (TLS sockets, screens that fit another size, libraries of any size), so plugins change
once: from interface 4 of 0.5.0-beta.12 to interface 6. Interface 6 was never released; plugins built for it (the
earlier release candidate's store files, test cards and Workshop builds) are refused as built for an older
interface, and are thrown away.

Every plugin is built anew for interface 6, so the split of the library's folders (#280) needs no rule of its own any more: a build
from before the split is refused with the rest.

### In plain words

This is a beta. Keep a copy of your books. The details for plugin authors follow in the sections below.

- **Per-book data lies elsewhere now** (#280). What PaperPaw keeps for each book (its details, its small cover, the
  caches of the format plugins) is spread over 256 subfolders by the first two digits of the book's key, no longer
  in one folder of thousands. A big library opens much faster on a slow card. After the update PaperPaw moves the
  old data once, a few books at a time at calm moments; each book's first opening takes a little longer, as its
  cache is made again. **Plugins that read the library have to be built again for it**: calibre, end-of-book,
  file-manager, files, the format plugins (EPUB, FB2, HTML, Markdown, CBZ, DOCX, MOBI, TXT, XTC, images), home,
  notes-overview, opds, sleep, stats, transfer and widgets. PaperPaw's own are rebuilt for this release; a plugin
  from someone else that reads the library needs its author's new build, and until then shows books without their
  details.
- **The room hint after an SD install.** After an installation from the SD card (as from CrossPoint) there is less
  room for plugins than a full flash gives. The store and the plugin manager now say how much room is left and
  that *A full install via USB (paperpaw.org/flash) gives about …* more; on a fully flashed device they say nothing.
- **Neutral plugin names** (#90). Plugins that work with a third-party product have a generic name; the product is
  named only in the description, as what they work with: *Wireless library* (calibre), *Book notes sync* (Joplin),
  *Highlights sharing* (KOReader), *Highlights export (cloud)* (Readwise), *Read-later articles (own server)*
  (wallabag), *Read-later articles (cloud)* (new id `read-later-cloud`, off at first), *Encyclopedia lookup*
  (Wikipedia), *DOCX documents*, *MOBI books*. The ids of the plugins already published stay.
- **Arabic user interface** (#33). The screens run right to left in Arabic: the switch, the rows, the keyboard, the
  library, the reader's menu, the sliders and the taps and swipes at the sides. The translation comes as the data
  plugin `language-ar` from the store. Nobody on the team reads Arabic: the rules are tested and the pictures
  checked, how it looks to a reader and how it feels on a device are not. Tell us what is wrong.
- **A wider keyboard** (#91). The keys reach nearer to the sides of the screen; on small screens they get nearly the
  whole width.
- **The landscape home screen** (#88). Turned on its side, the home screen keeps the start card beside the clock. On a wide
  screen it shows the same items with more columns, each page filled before the next, and cards no wider than about
  110 mm; arranging it on its side keeps that layout for that orientation, and *Reset landscape layout* (in Add)
  goes back. The SDK gets `grid::refit` for it.
- **A power menu** (#78). Hold the power button: *Sleep*, *Switch off*, *Restart* and, on touch devices, *Lock touch*
  or *Unlock touch*. A short press still sleeps.
- **All six language packs are in the store**: Arabic, Hebrew, Greek, Cyrillic, Korean (2,350 syllables) and
  *Korean, more syllables* (about 30 MB on the card). The Arabic, Hebrew and Korean packs bring their keyboards
  (Hangul letters join to syllables as you type).
- Also new: *Literata bold and italic* come as a small pack from the store (531 KB less firmware, more room for
  plugins), *All notes*, *End of book*, *Pictures* and the formats MOBI and DOCX, flash cards from the card and from
  Anki decks, your own fonts, hanging punctuation, drop capitals, lists that hang under their marker, line breaking for
  Korean, Chinese and Japanese. Book text in Arabic and Hebrew is set right to left.
- **Libraries of any size** (#79). The X4 no longer stops at 2,000 books: the library lists every book, paged from a sorted
  list PaperPaw keeps on the card (a few kilobytes of memory however many books; 10,000 books were tested on a
  computer). Right after the catalog is made anew the list says *Preparing the library …* for a moment. The shelves
  (authors, series, tags, and now folders) show 200 names at a time and go on when the focus moves past the last. A start
  after nothing changed writes next to nothing to the card, and big folders are read about twice, not once per piece.
  The first walk of new books writes about a sixth of what it did (#98), because an EPUB is opened for its details only.
- **Read while the library is collected** (#108). The first time the library is collected (thousands of books copied
  onto a new card), the books found so far are listed after the first 20, then each time they are twice as many, and
  every two minutes at least: they can be opened and read at once (the collecting waits meanwhile and goes on when the
  book is closed). The count says *1,240 books so far …* until all are found.
- **The quick panel starts at the top edge** (#120). It no longer leaves a band above it, which showed what was
  below: over a book the first line of the page, cut. Over the home screen it now covers the status line while it is
  open, as it covers the rest.
- **Look for new books** (#109). An empty library has a button that looks for books again, and the library's menu has
  the same entry: for books copied onto the card while PaperPaw ran. While it looks, the library says it collects the
  books and shows them as they are found ("N books so far"), as on the first start; pressing it again lets the search
  go on. **After swapping the SD card, restart the reader**: PaperPaw reads the card it started with.
- **Arrow keys stay on the page** (#112), a new setting under *Controls*, off by default. On, the arrow keys go
  round on the page shown: back on its first entry goes to its last, on from its last to its first (on a short last
  page among the entries it has); only the page buttons turn pages. In grids (covers, the home screen's tools and
  widgets), up and down go round in the column: up from the top row to the lowest entry below, down from the bottom
  row to the top. Off, as before, they go round the whole list.
  The library's Find screen follows it too: up and down (and the page buttons) go round the search and the letters in
  the column the focus left.
  For the lists of PaperPaw's own plugins; store plugins follow once they are built again.
- **Sharpness** (#122), a new reading setting under *Reading > Display*, next to the text contrast: *Soft*, *Normal*
  (the default, drawn as before) or *Sharp*. Soft draws letters a step lighter (smooth where the screen shows grays),
  sharp a step darker and without grays on every screen; each book may have its own. A tone curve set by a theme
  (`core.text-curve`) still takes its place.
- **Sharper letters** (#122). The font packs of the default fonts are built with their letters fitted to the pixel grid
  (x-height, baseline and stems on whole pixels), so text looks even and crisp on the e-ink screen. Letter widths and
  line metrics are as before, so a book breaks into the same pages. Packs on the card (Literata styles, scripts) are
  not changed.
- **Secure connections for plugins** (#81). Plugins can open HTTPS, IMAP and SMTP connections: the X4 does the encryption
  itself (one connection at a time), the emulator and the Android app use rustls. A server whose certificate no authority
  vouches for is shown and trusted only after you say so, and then only that certificate or key. The X4 just after a
  cold start does not know the time yet and says so. The first plugins that use it are *Mail* (IMAP/SMTP with a
  password or app password, up to three accounts, home widgets) and *Notes vault* (below), both from the store.
- **Background connections** (#81). A connection that background work opens (a mail fetch) stays open until that work
  ends; it no longer closes when the screen that happened to be showing closes.
- **Notes link to notes in the reader** (#87). Markdown notes open with `[[wikilinks]]`, links between note files and
  headings (*Back* returns to the note before), embedded pictures, hidden front matter, callouts and tags. The *Notes
  vault* plugin (from the store) adds a vault view of a notes folder and six home widgets. It works with Obsidian
  vaults and does not change them.
- **Screens that follow the turn** (#88). Screens lay themselves out again when the screen turns or changes size,
  keeping the focus, instead of staying as they were at the start.
- **Smaller menu text by default** (#119). The interface text (menus, lists, settings) starts at 8 pt, on every device
  (it was 9 pt). Anyone who set another size keeps it (Settings, Look); the reading size is unchanged.
- **Search finds tags, and stays small** (#111). The library search now matches tags too, as its question says
  ("Title, author, series or tag"). A search that finds over 500 books lists the first 500 by title and says
  "First 500 of N books"; narrow it with another word. The Find screen starts on the search field, and Up from the
  first row of letters goes back to it.
- **The keyboard's Right key** (#111). With buttons, Right on *Phone* (the field's last button) goes straight to the
  first key.
- **Plugins go from interface 4 to 6 in one step.** Every plugin with code needs a new build for this release (PaperPaw's
  own are rebuilt); a plugin from someone else needs its author's.
- **Updating:** books, reading positions and settings are kept. Every book is laid out again once, the first time it
  is opened. Plugins with code need a build for plugin interface 6; the store offers PaperPaw's own.
- **Not in this release:** exFAT cards. The X4 Pro, X4 Classic and X3 are built and checked by the tests on a
  computer; the owner tests the X4 and X4 Pro by hand.

### Added

- Settings: `builtin::settings::TEXT_SHARPNESS` (`core.text-sharpness`) and `TEXT_SHARPNESS_LEVELS` (soft, normal,
  sharp; #122), registered by the kernel with the reading text's look, each book may have its own. The SDK's reading
  faces move the contrast of `TEXT_CONTRAST` a step lighter (soft) or darker and without grays (sharp); normal and a
  value it does not know draw as before, and `TEXT_CURVE` takes its place. New constants, no change of what plugins
  built before see.
- Text: `paperpaw_text::Hinting` (none, light, normal, mono; `ALL`, `parse`, `name`) and, with the `outline` feature,
  `OutlineFont::rasterize_chars(chars, size, hinting)` and `build_pack_hinted(font, sizes, chars, hinting)` (#122): packs
  made on a computer with their letters fitted to the pixel grid; advances, kerning and line metrics as unhinted, so
  books lay out the same. `build_pack` and `rasterize` stay unhinted. `paperpaw-fontgen --hinting <none|light|normal|mono>`
  (unhinted unless asked); the default fonts are built light (`PAPERPAW_FONT_HINT` to compare). New items, no change of
  what plugins built before see.
- UI: focus moves that follow *Arrow keys stay on the page* (`builtin::settings::ARROWS_STAY_ON_PAGE`,
  `core.arrows-stay-on-page`, #112): `ListView::navigate_in` and `move_focus_in`, `Rows::step_in`,
  `Sections::step_in` and `ui::step_focus_in` (for a focus of a screen's own over pages of `page` things) take the
  `Env` and go round within the page where the reader chose so, else as `navigate`, `move_focus`, `step` and
  `step_focus`, which stay as they were. In a grid `move_focus_in` takes a step of a row (as many as it has columns)
  round within the page too, in the same column. New functions and a new constant, no change of what plugins built before
  see; the SDK's lists and PaperPaw's plugins use them.
- UI: `UiStyle::flip` and `UiStyle::flip_x` are public (#33): a screen lays a row or a grid out once from the left
  and mirrors each part where the interface runs right to left, as the SDK's own parts do. New functions, no
  change of what plugins built before see; their api.txt lines are part of this release's interface 6. The library's rows and
  covers, the reader's menu bar and the home screen's drawer mirror with them now, no copies of their own.
- Text: `TextInk::RIGHT_TO_LEFT`, a flag of the reserved ones: the lines a handle draws run right to left
  (`TextInk::is_right_to_left`). The kernel sets it for the interface's font where its language runs right to left.
- **Text of several lines** (#81). The keyboard can ask for a message of several lines (`ask::text(…).multiline()`):
  it has an *Enter* key, the field shows the end of the text on as many wrapped lines as fit above the keys, and a
  text from a phone or the clipboard keeps its line breaks. (A plain question still gets one line.)
- **Mail (#81):** a plugin that reads and sends e-mail over IMAP and SMTP with a password or an app password, no
  browser login. Up to three accounts under Settings, Mail, with presets for ten providers that fill in the
  servers; the folders with how many mails wait, a folder's latest mails, a mail (text, or the text of a simple
  HTML mail; attachments are named, not fetched), *Reply*, *Reply all*, *Star*, and writing a mail on the keyboard.
  It connects, fetches and closes (the X4 holds one secure connection), keeps what it fetched on the card, and asks
  once before trusting a server no authority vouches for. Five home widgets: unread mail, latest mail, starred
  mail, one mailbox and a shortcut to write. Needs the TLS sockets of plugin interface 6; background
  fetching and a Sent copy are not in this first version.
- **Notes vault (#87):** a plugin that reads a folder of Markdown notes (works with Obsidian vaults) without
  changing it: the vault's folders as they are on the card, its latest changes, its tags and a search in the
  notes' titles, each list shown a hundred rows at a time so that a vault of thousands of notes costs a window of
  memory (5,000 notes tested), from an index kept on the card and made in the background. Six home widgets: recent
  notes, a pinned note, the note of the day, a random note, the tags and a search, each with its own settings
  (how many, which folder). Notes open in the reader: `[[wikilinks]]` (`[[note|words]]`, `[[note#heading]]`) and
  links to other note files open that note by its name, anywhere in the vault, at its heading, and *Back* returns
  to the note before; embedded pictures (`![[picture.png]]`) show, front matter is hidden (its tags are shown),
  callouts (`> [!note] Title`) are quotes with their title, `#tags` stay as written and tables are rows of cells.
  The Markdown reader's HTML anchors the headings for those links.
- **Plugin interface 6:** `library::sorted` (#79): every book of the library sorted by title on the card
  (`.paperpaw/library/sorted-<hash>.idx`, one for each archive folder a view leaves out, two at most), paged by a view
  in a few kilobytes, so the X4 lists every book instead of 2,000 at most. `Sorted::open` (`None` for a file of
  another library version or archive, cut short or damaged: a view asks for it with `sorted::request` and says the
  library is being prepared), `len`/`page`/`find` for a `View` (a filter, the books read last first or by title,
  series stacked; as `Index::order` and `Index::stacked` list them), `next_in_series`, `refresh`, `hide`/`hide_all`;
  the event `SORTED_WANTED`. The kernel makes the file (kernel feature: `sorted::Build`, a bounded merge sort of runs
  of 512 books, written beside it and put in place whole; `sorted::wanted`, `asked`, `kept_archives`, `clean_up`).
- **Plugin interface 6:** `Storage::list_from` lists a folder a piece at a time, each piece going on
  where the last one stopped (#79): a folder of thousands of entries is read about twice in all, not once a
  piece. Storages that cannot go on from a place count, as `paperpaw_platform::list_counted` does (the item's
  default), so every storage keeps working; the memory card's (FAT) goes on from where the entry begins and
  finds a place taken by another entry by its count. The catalog lists the library's folders so: the walk after
  a start reads a folder of 4,000 books (1,254 blocks) in 2,535 blocks instead of 66,460 (bench; the whole walk
  4,367 instead of 68,291).
- **Plugin interface 6:** TLS for plugins' sockets (#81), as a mail plugin needs it (IMAP, SMTP):
  `Command::connect_tls` is secure from the start, `Command::start_tls` switches an open socket (STARTTLS) once
  everything the server sent was read, else it fails and closes. Nothing is sent or read until it is secure: a send or
  read before fails `NotReady` and closes the socket. `Tls` sets it up: the authorities PaperPaw ships (`Tls::new`),
  or up to `MAX_PINS` pinned certificates (`Tls::pinned`, `CertificateHash` as browsers and `openssl` write it) or
  keys (`Tls::pinned_keys`, `KeyHash` of the SubjectPublicKeyInfo: renewals of the same key stay trusted), the name
  checked, the time to connect, the time idle before it closes. `SocketNotice::Secured` and `Untrusted` (reserved
  before) carry the server's certificate and key, `Untrusted` why (`Distrust`); `Secured::time_checked` is false
  where the device did not know the time yet, so the certificate's dates were not checked (an X4 just after a cold
  start). `SocketNotice::Failed` says why it failed (`SocketError`; `Other(0)` from older kernels).
  `SocketRead::Bytes` reads bytes as they come (`SocketNotice::Bytes`), for 8-bit protocol lines that are no
  UTF-8. `Capabilities::tls_sessions` says how many TLS
  connections fit at once, HTTPS among them; with none (the browser's emulator) TLS sockets fail at once,
  `SocketError::Unsupported`.
- **Sockets of background work are the run's** (#81, owner decision 52): a socket, plain or TLS, that a work's event
  handlers or a job's step open no longer closes with the screen that was showing then (a run is no screen's: a
  mail fetch begun while a book was open lost its connection when the book closed). It closes when the run ends:
  the job done or failed, the work run again (an event's run ends with the next), taken back or refused, or its
  plugin gone. Only the work's own plugin's sockets count; one connected from a socket's event is the screens'
  (several connections are a job's steps). A secure one ends when idle: `idle_ms(0)` is refused for it
  (`SocketError::Options`); `MAX_TLS_IDLE_MS` is 30 minutes (was 10), so that waiting for new mail (IMAP IDLE)
  keeps it open. Sockets a plugin opens for the reader still close with their screen.
- **Plugin interface 6:** screens fit another size in place (#88). `Screen::on_resize` lays a screen out
  anew when its part of the screen changes size (turned, a window resized, the row of hints): answering
  `Handled::Yes` it keeps everything (a book open, a list scrolled, a connection); by default it is made anew as
  before. The old screen is now dropped before the new one is made, so a turn never holds both in memory.
  `Screen::pane` says `Pane::DETAIL` for a screen that shows one thing of the list below it, to show beside it on
  wide screens (the placement comes later); `Pane` is a number with constants, as `Instances`, and a value a kernel
  does not know shows as `Pane::FULL`. `Screen` grows at its end from now on: `Screen::revision` (never
  implemented by plugins) says which items a screen has, and newer ones are called only through
  `call_since`, so later screen items need no raise.
- **X4, X4 Pro and X4 Classic: TLS for plugins' sockets** (#81): a socket made secure from the start or switched
  to it (STARTTLS) is an mbedTLS session over the same connection HTTPS uses, the authorities of the firmware
  (Mozilla's) or up to four pinned certificates or keys (the server's own, by the SHA-256 of its DER or of its
  public key), the name asked for (SNI) and checked, the dates checked once the time is known (`Secured` says
  whether they were). The X4 holds one TLS session at a time and the X4 Pro and
  Classic two, shared with HTTPS and served in the order asked: a socket that waits longer than its timeout fails
  `Busy`. Nothing is sent or read before `Secured`; a switch to TLS over bytes that came after the server's
  go-ahead fails (`NotReady`, socket closed), as they might be injected.
- **Emulator on Linux, macOS and Windows, and the Android app: TLS for plugins' sockets** (#81, owner decision 52):
  rustls with ring, over the same sockets, with the same rules as the X4's. The authorities are Mozilla's list
  (`webpki-roots`, the same as the X4's, not the computer's store, so a plugin behaves alike everywhere), or up to
  four pinned certificates or keys; the name is checked for the authorities only, the dates (where the clock
  is believed, i.e. set after 2025) for both; there is no way to trust what is merely shown. A server's own
  certificate, a certificate authority's that it made itself, ends `Untrusted` (`UnknownIssuer`), so a plugin can
  offer to pin its key. Up to eight sessions at once, a socket waits for one until its timeout; a write that moves nothing for 30 s ends the socket, `Timeout`, so a server that never reads cannot hold a session. The Mail plugin
  checks and sends mail against a real IMAP and SMTP server this way. The browser's emulator has no sockets and
  holds no TLS session.
- Screens mirror in a user-interface language that runs right to left (#33): the switch, value rows and their
  chevrons, ticked rows, the number buttons, text fields, dialog buttons, list rows' icons, chips, the date
  and time pickers, headings and the reader's menu bar start at the right, and left/right follow them. The
  shared UI functions changed for this: `switch_rect` is the one whose partner stays a copy in plugins
  (ADR 0026 case 3), which the interface raise of this release makes moot, as every plugin is rebuilt.
- A book in Greek, Cyrillic, Arabic or Hebrew whose letters the reading font lacks names the pack that brings
  them, once for each script (to become one dialog with *Download* once for each book).
- Keyboards for Arabic, Persian, Urdu and Hebrew come with their letters packs (`script-arabic`,
  `script-hebrew`, 1.1.0), and a two-set Korean keyboard with the Korean pack: letters typed join to syllables
  (jamo), *Delete* takes back one jamo and *Shift* types the doubled letters (#61, keyboard 1.0.2).
- SDK: `library::kept_path(folder, prefix, book)`: where a plugin keeps what it keeps for a book in a folder of
  book data, in the part of the folder of the key's first two digits (`<folder>/<2 hex>/<prefix><key>`), so that
  no folder holds thousands of entries. The format plugins (EPUB, FB2, HTML, Markdown, CBZ, DOCX, MOBI, TXT, XTC,
  images) keep their caches so.
- SDK (document): a table cell can span rows (`TableCell::rows`) and say where its lines sit
  (`TableCell::vertical`, `VerticalAlign`); `TableRow`, `TableCell` and `MAX_COLUMNS` are in `paperpaw_sdk::document`
  for format plugins.
- SDK: `Command::RequestPlugins` (`Command::request_plugins`): any plugin can ask for plugins by id (e.g. those
  a template names); those not on the device go to the store as `events::PLUGINS_REQUESTED`
  (`PluginsRequested`, at most `MAX_REQUESTED_PLUGINS`), which asks the reader before it downloads any. What
  came of it reaches the plugin that asked as `events::PLUGINS_ANSWERED` (`PluginsAnswered`: installed,
  missing, declined, an error to show), from the store, or from the kernel where all were there already or
  no plugin installs plugins.
- SDK: `Settings::default_from` names the plugin whose default a setting has in place of its own (a template's
  `settings.ini`, a theme's), so a settings page can say where a value comes from.
- Font packs (`.ppfp`) can lie loose in a shell's font folder, without a `plugin.ini`: each is a face of the
  family its file name gives (`Literata-BoldItalic.ppfp`, `Bookerly.ppfp`). A device without scalable fonts
  leaves OpenType/TrueType files there alone (no warning for each); hidden files (`._…`) are passed over.
- SDK: `Fonts::missing_letters` (and `FontSet::missing_letters`) counts the letters drawn that no font has, so
  a screen can tell that a page lacked letters (e.g. to offer the font pack with them).
- SDK (document, layout VERSION 11): a run's style can carry the book's letter spacing
  (`TextStyle::letter_spacing`, in fortieths of an em, `LETTER_SPACING_STEPS`), a line over the text
  (`TextStyle::OVERLINE`) and how its lines are drawn (`TextStyle::line_style`, `LineStyle`: one, two, dotted,
  wavy); the reading faces measure, lay out and draw them. A drop capital's line is marked
  (`LineBidi::DROP_CAP`).
- Numbers in messages are written with the digits a language's catalog names (`[meta] digits`, e.g. `arabext`
  for Persian, `arab` for Arabic-Indic). SDK: `Translator::digits`, `Translator::number`,
  `Translator::localize_digits` for numbers a plugin writes itself.
  What the SDK writes for showing uses them too: the clock (`time::clock_of`, `clock_text`),
  `time::duration_text`, the battery percent in the status line, a number setting's value
  (`ui::setting_value`) and the date and time pickers. New: `files::shown_size(env, bytes)`, a size
  with the language's decimal mark and digits (`files::size` stays Latin, with the mark given).
- SDK: `Slider::mirrored` for a right-to-left interface: the lowest value at the right, the track filled from
  there, `−` at the right and `+` at the left (`Slider::parts`), the left button moving it up
  (`Slider::key_step`); `SliderPart::mirrored` tells a theme's painter.
- SDK, reserved for features known to come, so they need no new plugin interface (no effect yet):
  `Command::PlaySound` (`sound::Sound`, reply `replies::Played`, answered at once, not played) and
  `Command::StopSound`; `Capability::CHORDS` (key maps may bind `chord.<button>_<button>`, read and kept),
  `Capability::PAGE_TURNER`, `Capability::HOTSPOT`; `paperpaw_table::Family::CORE` for the firmware's own
  `core`; a spare word (zero) in `Manifest`, `ScreenSpec`, `FixedScreen`, `WidgetSpec`, `FixedWidget` and
  `StatusItemSpec` for flags later releases give a meaning with setters of their own.
- SDK: room for later questions without a new plugin interface: `KeyBindings::extended`,
  `ScreenMaker::extended` and `Typesetter::extended` (with `paperpaw_layout::TypesetKey`) answer `None`
  by default; none is asked yet (`docs/sdk-design.md`, rule 17).
- In a right-to-left interface (Arabic), taps at the sides and swipes go the other way on every screen, as
  they do in a book whose pages go right to left. SDK: `Screen::direction` (a `lang::Direction`) replaces
  `Screen::mirrored`: `Auto` by default (as the interface goes), `Ltr` or `Rtl` for a screen with a way of
  its own; the reader gives its book's, so a left-to-right book keeps its sides in Arabic. **Breaking** for
  screens that implemented `mirrored`.
- Korean written in jamo (Unicode's decomposed form, NFD, as some books and Mac file names have it) shows as
  its syllables wherever a font holds them, in books and in the interface, instead of loose jamo or question
  marks; letters so written no longer count as missing. `paperpaw_text::has_jamo` tells such text (#61).
- SDK: `CellGrid::mirrored` for a right-to-left interface: rows of cells start at the right, taps find them
  there, and the left button goes on to the next cell.
- SDK: rows moved by dragging have their grip at the left in a right-to-left interface.
  `Reorder::grip` and `Reorder::content` take the `UiStyle`, and `Reorder::draw` takes `&mut self`
  (it notes the direction for the pointer samples that follow): **breaking** for plugins that move rows.
- A language can come compiled from the card and be read as it is needed (ADR 0033): a data plugin's
  `i18n/<lang>.ppcat` (made with `paperpaw-sign catalog <lang>.ini <lang>.ppcat`, data formats 4) keeps its
  messages on the card, about a kilobyte of memory for 3,400 messages, and the messages of the screen shown;
  an Arabic interface of all plugins (some 210 KB) fits the X4. A pulled card or a damaged file falls back to
  English. SDK: `Catalog::compile`, `Catalog::open`, `CatalogFile`, `Translator::forget_read`,
  `Registrar::add_translation_file`; plugin interface 6 is written anew for it (every plugin is built again),
  `DATA_FORMATS` is 4.
- When the library's folder cannot be read (a failing card, one the device cannot use), the library says
  *Couldn’t read the books folder* instead of collecting books for ever or saying there are none; PaperPaw tries
  again by itself. SDK: `events::CATALOG_FAILED`.
- SDK: `WifiState::Online` says the name the device answers to in the network (`name`, e.g. `paperpaw.local`,
  where the shell gives one), and `WifiState::reach` gives the name or else the address for a page to show.
- SDK: `HttpRequest::max_file` caps what one download (`save_to`) may write; a longer answer fails and nothing
  is kept, so a plugin's download cannot fill the card (at most 64 MiB, as before).
- The store and the plugin manager name the web flasher for more plugin room only where the X4's shell says a
  full flash gives more (a smaller layout than PaperPaw's own, as after an SD install from CrossPoint), no longer
  guessed from the room's size. SDK: `CodeRoom::larger`, `CodeRoom::in_slot` (kernel feature).
- *Automatic* power wears *Saving* while the system's own battery saver is on (where the shell reports it). SDK:
  `Capabilities::power` (`PowerControls`: what the shell changes by the level, so settings show only what
  applies), `KernelBuilder::power`, `Battery::saver`.
- SDK: `PluginUsage::shown_ms`, `read`, `written` and `awake_ms` are `u32` (they stop at their largest value):
  a row is kept for every plugin, about 1.3 KiB less on the X4.
- SDK: `Str`, text kept in the program or owned in two words (a `Cow<'static, str>` takes three);
  `Manifest::name`/`description` and `SettingSpec::title`/`description` are `Str`, as the kernel keeps them for
  every plugin and setting (16 bytes less per plugin on 64-bit, 8 on the X4).
- SDK: widgets kept in the program, `FixedWidget` and `Registrar::add_widgets` (a `static` list, two words of
  memory for the list where each `WidgetSpec` takes about a hundred bytes); `Drawn::new`, `shows` and `targets`
  are `const`. Home, stats, vocabulary and six of the widgets plugin's widgets use them.
- SDK: subscriptions kept in the program, `FixedSubscription`, `Registrar::add_subscriptions` and `hears!` (a
  typed handler, as `Registrar::on` takes it, in a `static` list: one entry of the kernel's for the list);
  `Event::hear`, `Event::id_static`. The update plugin's seven subscriptions use them.
- SDK: `Document::prepare_image_larger` decodes a picture anew, larger than for its page (its longer side up to
  1600 pixels), for a zoom or a picture on the whole screen; EPUB books do, other formats say they cannot.
- A plugin on the card can name the plugins it needs: `requires` in `plugin.ini` (data formats 4), e.g. a setup
  template. SDK: `requires::parse`, `requires::of`, `requires::missing`, `Requirement`.
- SDK: hooks for showing the card to a computer as a USB drive (the X4 Pro's hardware can):
  `Capability::USB_DRIVE`, `Command::UsbDrive`, `events::USB_DRIVE_ENDED`; kernel: `Effects::usb_drive`,
  `Kernel::usb_drive_ended`, `KernelBuilder::capability`. No shell offers it yet.
- In *Offline* a plugin can offer *Go online* outside its network screens too (a widget's *Update now*): SDK
  `Command::GoOnlineFor` (the network for a while, at most `power::MAX_ONLINE_FOR`, ten minutes; held work runs
  as in Saving), taken only from the reader's input.
- Author keys: a recovery key chosen in advance moves a plugin to a new key by a statement the device checks
  itself (`paperpaw-sign rotation`; SDK `native::keys`: `remember_first_install`, `rotation_message`,
  `accept_rotation`; `install::Marks::recovery`), and the code of an author whose key the store revoked does not
  run (`PluginProblem::Revoked`, `keys::keep_revoked`, which only adds). A revocation is a statement
  (`keys::Revocation`, `paperpaw-sign revocation`) signed by the revoked key itself or by the plugin's recovery
  key, about the key pinned as its author on the device; any other is left out, and the list is checked again where
  it is read, so no taken-over library can revoke another author's key (installed plugins are trusted: they have the card). A revocation stops that plugin
  alone (`keys::is_revoked(storage, plugin)`), not others the same key signed. The recovery key is pinned only with
  the first install, and code installed before a rotation does not run until an update by the new key: install marks
  never move a known author to another key, and name code as installed only where the author pinned then signed it. The store
  does not offer either yet; it calls `keys::keep_revoked` with its library's revocations and `keys::accept_rotation`
  with a statement that came with an update (both plugin-facing; the recovery key arrives with the install marks;
  `keys::check_rotation` says beforehand, without writing, whether a rotation would be taken).
- Reserved for devices and plugins to come, so they need no new interface: a pen above the screen
  (`PointerPhase::Hover`, which the kernel passes to no screen yet) and how it leans (`PointerEvent::tilt_x`,
  `tilt_y`, `with_tilt`; `Capability::PEN_HOVER`, `PEN_TILT`); TLS sockets (`SocketNotice::Secured`, `Untrusted`
  with the certificate's SHA-256; `socket_params::SECURED`, `UNTRUSTED`, `CERTIFICATE`), whose commands will come
  as `Command::Extended` (#81). The layout digest now also holds where `PointerEvent`'s fields lie (its size hid
  the new ones), so the plugin fingerprint changed: build plugins from the baseline below or later.
- Layout `VERSION` 11: books cached before are laid out anew once, so drop capitals and the new list markers show
  in them too. SDK, for real tables: `BlockKind::TableRow`, `Block::table` (`TableRow`, `TableCell`, `MAX_COLUMNS`:
  a row's cells, the table's column weights, header, first and last row) and `Page::rules` (`PageRule`: thin lines
  such as a table's borders). Formats and the layout do not fill them yet: rows are still set as one line.
- The start asks for the plugin list and the translations' list once at their size (SDK, kernel feature:
  `Plugins::reserve`, `Translator::reserve`), so they do not double while plugins register.
- The English catalogs of the plugins built in are one catalog, indexed together when PaperPaw is built: the
  translator keeps a table of their sources rather than a catalog for each. SDK: `BuiltCatalogs`, `BuiltTable`,
  `Translator::built_catalogs`; kernel: `KernelBuilder::built_catalogs`; `paperpaw_app::BUILT_CATALOGS`.
- A power menu: hold the power button and a small menu opens with *Sleep*, *Switch off* (where PaperPaw can switch
  the device off), *Restart* and, on touch devices, *Lock touch* or *Unlock touch* (power 1.1.0). A short press
  still sleeps, now when the button is let go. The touch lock no longer binds the held power button itself (so the
  menu can open while touch is locked); it keeps the held home key and the two taps with a hold, and its hint says
  to choose *Unlock touch* in the power menu (lock 1.0.3). No SDK change.
- *Lock after waking up* (the screen lock's setting, on by default) is no longer an advanced setting: off, the lock
  comes only after a real start (power on or restart), not after sleep.
- *All notes*, a new plugin (Tools): the books that have bookmarks or highlights with how many, a book's notes
  (a note opens the book there), and *Export as My Clippings.txt*, all notes in one file in Kindle's form
  (UTF-8 with a byte order mark, `Exports/My Clippings.txt` beside the library) that notes apps read. A book's
  notes file is found by the hash of its path, so listing needs no book to be opened. It needs no SDK change.
- *MOBI books* (`format-mobi`), a new format plugin: opens `.mobi` and `.azw` books without DRM (PalmDOC or uncompressed
  text, Windows-1252 or UTF-8). It shows the title, authors, language, subjects, the cover, headings with a table of
  contents, bold and italic, centred and right-aligned text, lists and pictures (GIF, JPEG, PNG, decoded when
  their page shows). It reads the book in pieces, so a long book needs little memory. Books with DRM, with
  Huffman/CDIC compression or in the KF8-only form (`.azw3`) are refused with a clear message. It needs no SDK change.
- *Read-later articles (cloud)* (`read-later-cloud`, #90), a new plugin (off at first): fetches the articles saved in the Readwise Reader service (from *Later* or
  the *Inbox*, a setting) as HTML books into the folder `Reader`, with their text (no pictures), ten new ones
  at a time. It asks for the Readwise access token once; articles read to the end are archived in Reader with
  the next fetch and their books removed (not those with highlights, notes or bookmarks). It keeps within
  Reader's limit of twenty list requests a minute and stops where Reader asks to wait. It needs no SDK change.
- *DOCX documents*, a new store plugin (`format-docx`): opens `.docx` files as books, with headings and a table
  of contents, lists (bulleted, numbered, nested), tables (a row a line), bold, italic, underline, links (also
  within the document) and pictures (JPEG, PNG, GIF), and the title, author and language of the document. The
  text is read in pieces from the archive and written once as blocks, so a long document needs little memory;
  pictures are taken out when their page shows. Footnotes, headers and footers, and comments are not shown.
  It needs no SDK change and is not part of the firmware, so only readers who want it pay for it.
- *Pictures*, a new store plugin (`format-images`): a JPEG, PNG or GIF opens from the library as a book, one
  picture to a page, fit to the screen; the pictures after it in its folder (in the order of their names, 2
  before 10) are the pages after it, up to 300. Each picture is decoded when its page shows, so one is in
  memory at a time; a file that is no picture or too large shows its name. It needs no SDK change and is not
  part of the firmware.
- *Plugin layout and best practices* (`docs/plugin-conventions.md`): the folders of a plugin, names, where pictures and other assets go (on the card, not in the code), translations, data a plugin writes, and the budgets. `paperpaw-dev package` warns where a data plugin's folder or a code plugin's source (`paperpaw-dev package path/to/crate`) does not keep them. It needs no SDK change.
- *End of book*, a new plugin: when the last page of a book is reached, a screen offers the next book of its
  series (or the library), a rating of one to five (`4 / 5`, kept as a book field and shown as a badge on the
  book's row and on its information page) and whether the book counts as finished. Two settings in the reading
  group: *Show a screen at the end of a book* and *Mark the book as finished* (both on). It needs no SDK change.
- Flash cards from the SD card (vocabulary 1.1.0): every CSV or TSV file in `/Flashcards/` is a deck named after
  the file, and *Cards on the SD card* updates the decks from their files (cards match by an id column where the
  file has a `#paperpaw:` line, else by their front; cards deleted from the file stay unless the deck's *Remove
  missing cards* switch is on), or saves a deck back to its file (written to a `.part` file, the old one kept as
  `_<name>.bak.<ext>`, a change made on the computer in between merged first). Decks also come in through *Edit
  with your phone* (CSV, TSV, TXT or Anki `.apkg`), which the page, the help and the empty state now explain, and
  with errors that name the problem: a wrong file type, a file that is not UTF-8 or has no cards, a full deck.
  Files over 4 MiB or 20,000 lines are refused and a card holds at most 500 characters. No SDK change.
- Flash cards from Anki decks on the card (#49, part 3): an Anki deck package (`.apkg`, exported with *Support older
  Anki versions*) in `/Flashcards/` is a deck like a CSV or TSV file: the notes' first three fields (front, back,
  example) become cards as plain text, without markup and sounds, and an update merges them as it does a text file's
  lines. Scheduling and media are not read (ticket #26, Anki's own sync, is not part of this). The deck's database is
  read page by page, so the heap needs no more than for a text file. A deck of the newest Anki format, one with
  more than 4,000 files, a database over 16 MiB or more than 20,000 notes is told, not read. An Anki file is only
  read: *Save to card* never writes over it. No SDK change.
- A release's `paperpaw-<device>-<version>-abi.txt` says the firmware's table for plugins after its
  fingerprint: `interface:`, `api:` (the table's size), `families:` and `min:` (each family's revision and the
  oldest it runs), read from the firmware's ELF file (`PAPERPAW_NATIVE_ABI`, `paperpaw-sign abi`). The store
  and the web flasher use them to refuse a plugin that needs more than a firmware has; the first line is the
  fingerprint as before.
- Your own fonts (#59): *Your own fonts*, beside *More fonts* where the reading font and the status lines' font
  are chosen, shows how to read with a font file of one's own, per device, in English and German (it works
  without a network). `paperpaw-fontgen --family "My Font" --regular a.ttf [--bold b.ttf] [--italic i.ttf]
  [--bold-italic bi.ttf] <folder>` makes the whole font plugin for the X4 and X4 Pro, which read font packs
  only: `<folder>/font-my-font` with its `plugin.ini` and a pack per face, to copy into the card's `plugins`
  folder; it prints how large each pack is and how many of the characters asked for the font has. No SDK change.
- A Greek letters pack, `script-greek` (#60): Literata's Greek, modern and with the older accents (Greek
  Extended), for books whose Greek shows as `?` on a device whose fonts hold Latin only. It is a data plugin
  that `tools/script-pack.py greek` builds, with no code and no interface change. Its entry in
  `tools/plugin-files/data.toml` is live, so the release builds it for the store (off until chosen), with the Arabic, Hebrew
  and Cyrillic packs (`script-arabic`, `script-hebrew`, `script-cyrillic`): letters for books whose scripts the built-in fonts lack.
- A Cyrillic letters pack, `script-cyrillic` (#60), the same way for Russian, Ukrainian, Bulgarian, Serbian and
  other Cyrillic books (about 1.0 MiB on the card), off until chosen.
- A fix to one of PaperPaw's own data plugins (Literata's sizes, the
  hyphenation data; `tools/plugin-files/data.toml`) reaches readers alone
  between releases, as for the plugins with code: tagged `<id>-v<version>`,
  a pre-release with its own catalogue (`store-plugin.yml`).
- `paperpaw-sqlite`: `Database::scan_bounded` reads a table like `scan`, but a row whose payload is more than a
  given number of bytes comes as `Row::Oversized(size)`: nothing of it is allocated and its overflow pages are not
  read. One huge row (a picture kept in a note) then no longer fails the whole table for want of memory. Not part
  of the plugin interface.
- The store lists a built-in plugin that is switched off (Android builds the store's plugins in, off until wanted)
  with *Switch on*, which does what the plugin manager does; nothing is downloaded for it.
- Each release has `paperpaw-fontgen`, which makes bitmap font packs (and, with `--family`, a font plugin to copy to the
  card), built for Windows (x64), macOS (Apple silicon and Intel) and Linux (x64): `paperpaw-fontgen-<version>-<computer>.zip`,
  with its SHA-256 and a signature of its own, like the emulators.
- *Pictures in books* (Settings › Reading › Reading aids, in the `aids` plugin): show them as the book has them
  (as before), put a short line such as `[Picture: Rings of light]` in their place (the picture's description,
  centred, italic), or leave them out. No picture is decoded for a picture not shown. Comics and other books
  made of pictures (CBZ, XTC) are not changed. It needs no SDK change.
- A Korean letters pack, `script-korean` (#61): the 2,350 Hangul syllables of KS X 1001, which ordinary modern
  text keeps to, with CJK punctuation, compatibility jamo and full-width forms, drawn from Nanum Myeongjo (SIL OFL
  1.1; the pack is not named after it, as its licence reserves the name). One face in seven sizes, 8.2 MiB on the
  card, for books whose Korean shows as `?` on a device whose fonts hold Latin only. It is a data plugin that
  `tools/script-pack.py korean` builds from the repository, with no code and no interface change. It is listed in
  `tools/plugin-files/data.toml` for the store (publishing waits for the release); its keyboard
  layout (two-set Hangul, `tools/plugin-files/keyboards/korean.txt`) comes with it.
- A second Korean pack, `script-korean-extra` (#61): the other 8,822 of the 11,172 Hangul syllables (rare ones outside
  KS X 1001), about 30 MiB on the card and no flash. It only adds letters to `script-korean`, so it needs no interface
  change; `tools/script-pack.py korean-extra` builds it (`tools/fontgen/sets/hangul-rest.txt`). It is listed in
  `tools/plugin-files/data.toml` like the Korean pack, and the reader offers it only for a book that uses such
  syllables, with its size.
- The Arabic user interface (#33): `language-ar`, a data plugin of one catalog (`i18n/ar.ppcat`, ADR 0033) with all
  3,197 messages of the kernel and the plugins translated (`tools/plugin-files/languages/ar.ini`; `tools/language-pack.py`
  checks it and builds the plugin). The language packs `script-arabic`, `script-hebrew`, `script-greek`,
  `script-cyrillic`, `script-korean` and `script-korean-extra` are listed for the store; nothing is published before
  the release.
- Author keys (#20), store side: a plugin's recovery key, named by the library, is pinned with its author's key at
  the first install; an update signed by another key is taken if the library carries rotations (`rotations`, in
  order) the recovery key signed, which the kernel checks, and refused (`KeyChanged`) otherwise. The library's
  `revoked_keys` (`{plugin, key, signature}`) go to the kernel, which keeps those that hold; a plugin it stopped says
  so on its store page, with *Remove*.
- The transfer screens show the device's name in the network (`paperpaw.local`) beside its address where the shell
  gives one; the QR code keeps the numeric address.
- Downloads of the store, the update, the news and font lists and the read-later cloud plugin are limited to the size the library
  names (`max_file`), so a manipulated server cannot make a plugin read an unbounded body.
- The icon font has `alert-triangle` (+320 bytes), and a check (`tools/checks`, icons) fails where a plugin names a
  Tabler icon the font lacks; the *Unreviewed* line of the store had been drawn without its icon.
- A data plugin's entry in `tools/plugin-files/data.toml` may have `how` and `de.how`: its *How it works* text.

### Changed

- The Android app icon takes the website's colours: a dark green sheet with a paper-coloured paw.
- **Greek, Cyrillic and Korean titles in the library's list** (#387): the screens' texts are drawn in Source Sans 3,
  which has none of these scripts' letters on the X4, and the Hebrew and Arabic packs brought a face for it while
  these three did not, so a book's title showed as boxes in the list. `script-greek` and `script-cyrillic` 1.0.1
  bring Source Sans 3's letters (+0.6 and +0.25 MiB on the card), `script-korean` 1.1.2 a second Hangul face for
  the screens (+1.9 MiB). A store update, no firmware change.
- The first walk of new books writes about a sixth of what it did (#98): an EPUB is opened for its details only (title,
  authors, series, cover) and its package goes into the reader's cache the first time the book is read, instead of
  a folder and its files written for every book the library finds. On the bench's card (FAT32, clusters of 32 KiB)
  the first walk of 1,000 books writes 26,952 blocks instead of 154,558 and reads 71,974 instead of 153,425; the
  X4 test of 3,000 small EPUBs (about 100 books a minute) is to be measured again. Covers are still made for the
  library during the walk, in the book's cache folder with a stamp of the book: a book replaced on the card, or a
  folder a cut-off removal left behind, is cleared when the book opens.
- **The first walk shows the books found so far** (#108): where there is no catalog yet, the kernel's walk adds the
  books found to `catalog.txt` at checkpoints (20 books, then each time twice as many, at least every two minutes once
  50 more were found), whole lines a 2 KB piece a step, sorts them for the views and sends `CATALOG_CHANGED`; the
  walk then goes on. The catalog it ends with is the one it would have put in place, byte for byte; cut off (the
  power gone), the catalog left lists the books found, and the next walk completes it. The kernel's library answers
  `Library::extended` with the key `core.library-walking` (`walking` = `true` while the catalog is made anew); the
  library's count says *so far* then. No change of interface 6.
- **Look for new books** (#109): the library (`files`) has the action `files.look-again` (in its menu, and as a
  button on an empty library), which sends `library::request_catalog_anew()` (`request_catalog()` while a walk that
  shows its books as it goes is under way, so it is not started over). A walk over a catalog without books removes
  it first and goes as a first walk does (#108): its books are put into a catalog at checkpoints.
- **Packs name the letters they need** (#97): the Arabic interface (`language-ar` 1.0.1) says `requires = script-arabic`
  and the rare Hangul syllables (`script-korean-extra` 1.0.1) `requires = script-korean` in their `plugin.ini`
  (formats 4), and their descriptions and *How it works* say it in words. Nothing fetches the letters by itself yet:
  the store reads `requires` in a later release; until then the letters pack is installed by hand.
- *Right to left: the overview of every tool and the keyboard's header* (#33): where the interface runs right to
  left the overview's grid starts at the right (the first tool at the top right, the left button on to the next
  tool), and the keyboard's title is at the right, the hint of how typing ends and the field's *Phone* and
  *Paste* buttons at the left, typed text from the right.
- *Right to left: the library and the reader's menu* (#33): where the interface runs right to left a book's cover is
  at the right of its row with the title, author and progress running from the right, the row above the books starts
  at the right (the count and the search at the left), the continue-reading card and the grid of covers start at the
  right (the left button on to the next cover), and the reader's menu shows the book's title and chapter at the
  right and the page's percentage at the left.
- The library lists every book on the X4 too (#79), not 2,000 at most: the list pages the books sorted on the card
  (`library::sorted`) and holds a few kilobytes (10,000 books: 3.7 KB held, 8 KB at most while opened or paged, on
  the host; the index of 2,000 took 48 KB). Its order, filters, stacks of series as covers, the next book of each
  series and where the focus goes are as before. Right after the catalog was made anew the list says *Preparing
  the library …* until the kernel sorted the books (a moment); the index it kept while a book covered it
  (`.paperpaw/plugins/files/index.bin`) is removed. On the X4 the catalog is now walked and caught up while the
  library shows too (the reason it waited, the index's memory, is gone); still never while a book is read.
- The library's shelves hold a bounded part of the memory however large the library (#79): the shelves in the
  list show the first 200 series and folders on the X4 (2,000 on others) with an *All series* and *All folders*
  row after them, and count the authors up to 2,048 (*more than 2048 authors*); the screens of authors, series,
  tags and (new) folders hold 200 names at a time, the next taken when the focus moves on past the last. At
  10,000 books of 5,000 authors and 2,500 series, showing the shelves took 1.26 MB on the host (about 600 KB on
  the X4, more than its heap) and now takes 52 KB; the authors' screen 214 KB, now 25 KB. Each name is counted
  exactly; the authors' screen showed the first 2,000 met before, now every author, in order. The list keeps its
  index when the catalog is as it was.
- A catalog walk that finds the library as it was writes nothing (#79): it compares what it makes with the old
  catalog and writes a new one only from where they part (the lines before copied a piece a step), keeps the
  catalog and its generation where they never do, and keeps a folder listed in one piece in memory instead of
  in `walk.list`. The walk after every start at 10,000 books writes 975 blocks instead of 69,744 (bench); after
  10 books came, 11,350 instead of 71,096.
- `shape.txt` no longer holds what only the kernel builds (the SDK's `kernel` feature, ADR 0020): no plugin
  has it, so its types change with the firmware alone (ADR 0026, `tools/interface.py`).
- *Keyboard: wider keys* (#91): the keys' side margin is a quarter of the margin other screens keep (at least
  4 pixels), so on small screens the keys get nearly the whole width. Every layout, tapped or chosen with buttons;
  the text field keeps its margin, the key order and the focus are as before. Round keys are as high as wide, so
  the keyboard gets a little higher; it still stops at half the screen where the size allows more.
- Arabic in the interface (#33): labels, rows, buttons, dialogs and whatever else draws with `FontHandle::draw`,
  `draw_toned` or `measure` set Arabic letters joined, in the order they are shown, as the reader does (`draw_in`), and
  `FontHandle::wrap` breaks them as wide as drawn. A line of both directions starts at the right with the interface's
  font in a language right to left (`Wi-Fi متصل`: `Wi-Fi` read first), as its first strong letter goes with other
  fonts. Text left to right alone is drawn as before. Plugins built before take it up from the firmware (shared text
  functions); plugins with their own copies need a rebuild.
- *Store: a plugin rebuilt for a newer PaperPaw* is not offered to a device whose firmware lacks what the build
  needs (a newer table or revision of a family of shared functions, as the library lists it in `interface`,
  `needs_api` and `families` of the build). The page says *Needs a newer PaperPaw* in place of *Install* or *Update*,
  and what is installed stays as it is; before, such a build was offered and the firmware refused it at the next
  start. A library that lists none of it is read as before.
- Large libraries: the books' details and covers made small are kept in 256 folders by the first two digits of their
  keys (`.paperpaw/library/details/<h>/<key>.ini`, `thumbs/<h>/…`) instead of one folder of thousands, which a
  memory card searched one entry after the other for each name (the first catalog of 5,000 books grew with the
  square of their number). What earlier versions kept in `books/` and `covers/` is read there until the kernel
  moves it, once, a few at a time at calm moments after the start (each move whole or not made, cut off or not);
  the old folders go once empty. Folders of book data (`add_book_data`, `add_book_cache`) may hold their entries
  in the same subfolders (`<folder>/<2 hex>/<prefix><key>`, `library::kept_path`): the kernel moves, removes and
  cleans up both, and with the move takes away once what folders of caches hold in the folder itself (made again
  from the book when needed: each book's first opening after the update takes a little longer). On FAT
  (the X4's card), a folder of 256 entries or more read whole is looked through from where the last name was
  found (for up to 8 such folders at a time), so names taken out of it in its order take a few blocks each, deep
  in a folder of thousands too (where it was in the folder's chain of clusters is kept: the FAT is not read along
  the chain from its start again for each name);
  rust-fatfs, as patched, moves and removes an entry found without looking for it again. EPUB books look for one's
  own stylesheets (`styles.css`, `<book>.css`) only when a chapter is first needed, not when opened: the catalog,
  which opens every book for its details, no longer searches each book's folder for files mostly not there. With
  10,000 books the first catalog reads 2.4 million blocks of the card instead of 160 million. Plugins that read the
  library's details must be built from this change on (they read `details/`).
- *Literata's bold and italic* are no longer in the firmware of the X4, X3, X4 Pro and X4 Classic (531 KB less, more
  room for updates and plugins) but a small font pack on the card, the plugin *Literata bold and italic*
  (`literata-styles`, 0.5 MB, 0 flash). The web flasher puts it on the card with the firmware; after an update
  or an install from CrossPoint, the store fetches it once by itself as soon as there is a network (the one download
  it makes unasked; not in Offline mode, three tries at most), says "Added" once, and else shows a short hint. Until
  the faces are there, bold and italic words are set in the regular face. `literata-large` (36 to 48 px) now
  extends these faces. A face that a plugin binds to a role is now loaded also when the reading font is the
  default one (it was, before, only once its family was chosen).
- The names of all Tabler icons take 35 KB less firmware: a name that begins the next one (`a-b` in `a-b-2`)
  is kept only in it, and each name's place, length and character take 5 bytes instead of 8
  (`tools/icons/subset.py`). `icons::names()` and `icons::char` give the same as before.
- New reading setting "Hanging punctuation" (`core.hanging-punctuation`, off by default, in the layout settings): an
  opening quote that starts a line, and a stop, comma, hyphen or closing quote that ends a justified one, are set
  just outside the text, so its edges look straight (optical margin alignment). Lines break as before, so pages hold
  the same text and kept page indexes stay valid; only left-to-right lines hang. The layout does what the reserved
  `ScriptOptions::HANGING_PUNCTUATION` bit names; no SDK change.
- Drop capitals: a book's large first letter (an EPUB's letter floated left at 1.5 times the text size or more, or
  CSS `initial-letter`) is set over the paragraph's first two to five lines, which are set in beside it. Only
  paragraphs that run left to right and start with a letter (after an opening quote or two); a paragraph of fewer
  lines gets a smaller capital. Only where the font can be drawn that large: fonts of fixed sizes (the X4's font
  packs) show the paragraph as before. The layout does what the reserved `Block::drop_cap` names (no SDK change).
  EPUB chapters converted before keep no capitals until they are converted again. The paragraph's own size is no
  longer taken from such a letter (it was set as large as the letter before). Builds without scalable fonts (the
  X4 and X4 Pro firmware) leave the drop-capital layout out (`paperpaw-layout` feature `drop-caps`, which the SDK's
  `outline` turns on); they lay such paragraphs out as before, as they did anyway.
- Lists in EPUB, HTML and Markdown books: an item's lines hang after its marker (the second line starts under the
  text, not under the number), and an item that starts with a paragraph has its marker on that paragraph's line.
  CSS `list-style-type` (and the `list-style` shorthand) and the old `type` and `value` attributes are read: disc,
  circle, square, decimal, lower and upper alpha (or latin), lower and upper Roman, none; other values keep the
  list's own markers. The marker is followed by a no-break space. Fonts without the circle or the square (the X4's
  font packs) show a bullet. Chapters converted before keep their old markers until they are converted again.
- Line breaking follows Unicode (UAX #14) where the text needs it: Korean breaks between syllables, Chinese and
  Japanese between characters, never before a closing mark (。」) or after an opening one (「), and U+200B (the zero
  width space) is a place to break; it and the word joiner U+2060 are now zero width and not drawn (they took
  the width of a missing letter before). A line that breaks by word (`ScriptOptions::KEEP_ALL`), small kana that
  may start a line (`LINE_BREAK_LOOSE`) and breaks between any letters (`BREAK_ALL`, `LINE_BREAK_ANYWHERE`) are
  options of the layout. Layout version 10: every saved page index is made again once after the update (reading
  positions are kept). Text in Latin, Cyrillic, Greek, Arabic, Hebrew and Indic scripts is laid out as before,
  except around U+200B and the word joiner, emoji, and fullwidth characters. The line-break table is 4.8 KB of
  firmware (Unicode 17.0.0, `tools/linebreak-table.py`). Arabic shaping does not change: U+200B and U+2060..U+2064
  join nothing across them, as before.
- The slider of the book text's size (settings and quick panel) has the sizes that look different as its positions,
  evenly spaced; before, its track was the whole range, so the sizes a font pack skips made the knob move one step
  here and two there.
- Settings: the separate *Update* entry in the settings menu is gone; the *Updates* group shows the update page
  first, with its state (*New*). A group page shows a plugin's actions before its settings.
- New reading setting "Korean line breaks" (`core.korean-breaks`, `settings::KOREAN_BREAKS`): by syllable (default, as
  printed books) or by word. It applies to books in Korean only.
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
- X4, X3, X4 Pro and X4 Classic: a plugin may ask to switch the reader off (`Command::PowerOff`, where the device allows
  it): plugins save as for a sleep, then the power goes at once, without the light sleep before. Not yet tested on a
  device.
- **Plugin interface 6** (every code plugin is built anew
  for it; interface 5 was never released). Plugin files are of format 4: the header (116 bytes) says the
  plugin interface a plugin was built for, how much of the firmware's
  table it needs and the revisions of the families of shared functions
  it uses, and a plugin's entry is 32 bytes on the chips. A plugin built
  for an older PaperPaw says so ("Built for an older PaperPaw: update it
  in the store"), one that needs a newer PaperPaw too
  (`PluginProblem::OlderInterface`, `PluginProblem::NeedsNewerPaperPaw`,
  `Native::problem`). The firmware's table (`Api`) gains the revisions
  of the families of shared functions and a build name, and grows at its
  end from now on without a new interface. `paperpaw-plugin.x` makes a
  word per family of shared functions (`PAPERPAW_TABLE_0` to `_7`).
- Plugin interface 6, the plugins' side: each of the first-party code
  plugins that the store offers (33 of them besides the ones that had a
  newer version already) has a newer version, so that a device fetches
  its interface-5 build as an update; until then its old build waits
  with "Built for an older PaperPaw: update it in the store" in the
  plugin list. Data plugins carry no code and keep their versions. The
  texts of these two messages are in English and German.
- Plugin interface 6: types that grow later are `#[non_exhaustive]` and
  are built with `new` and `with_*` (e.g. `Run::new`, `LineRun::new`,
  `DeviceInfo::new`, `PluginInfo::new`, `Mark::new`); the SDK's event
  payloads, `Progress`, `Metrics`, `library::Entry`, the platform's
  `DeviceInfo`, `DisplayInfo`, `SystemStatus`, `PointerEvent` and more,
  the text crate's `FontInfo`, `LineMetrics`, `Glyph`, and enums such as
  `Rotation`, `ButtonState`, `PointerPhase`, `Chip` (a `match` needs a
  `_` arm). `screen::Handled` stays as it is.
- `TextStyle` of `paperpaw-document` is 32 bits (bits 24 to 27 a
  block's inline language); cached books and page indexes are made anew
  (EPUB cache version 30, a new `paperpaw_layout::VERSION`).
- `paperpaw-i18n`: `Catalog::digits()` returns the ten digits (`[char; 10]`),
  not a name; `[meta] digits` takes a CLDR numbering system's name or the
  ten digits themselves (`digits = ០១២៣៤៥៦៧៨៩`), so any system is data.
  `DIGITS` and `CALENDARS` are replaced by `digit_systems()` and
  `calendars()` (25 systems, all 18 CLDR calendars), and `[meta] plural`
  knows all of the CLDR's rules for whole numbers (Welsh, Irish, Scottish
  Gaelic, Breton, Manx, Slovenian, Lithuanian, Latvian, Romanian, Maltese,
  Icelandic, Bosnian/Croatian/Serbian, Cornish, Tachelhit, Colognian,
  Filipino, Tamazight, and `one-many-other`, `zero-one-many-other` for the
  millions of Spanish, Italian, French and Portuguese).
- Font packs of version 2 have a table directory (anchors, mark-to-mark
  anchors, line metrics per script, language forms, contextual
  substitutions, vertical metrics); packs of version 1 read as before.
  Glyphs without a code point (Indic conjuncts, Khmer and Burmese stacks)
  take keys in the private-use planes 15 and 16; the language forms and
  substitutions tables have a format (version 1: single, ligature and
  multiple lookups per OpenType feature, script and language; unknown
  lookup kinds are skipped), read once a shaper uses them.
- `FontHandle::draw_in` and `measure_in` (text in parts of several fonts)
  are shared functions: plugins use the firmware's copy, so the way text
  is split between fonts improves without rebuilding them. A mark (Thai,
  Devanagari, Khmer, Burmese or Tibetan vowel signs among them) and a
  conjoining Hangul vowel or final stay in their letter's font:
  `paperpaw_text::is_mark` is Unicode's general category M (it knew only
  the combining blocks).
- `PixelFormat::Other { code, bits }` and `RefreshMode::Other(code)`: a
  device's own pixel format or waveform (a colour e-ink panel's) without a
  new interface. No framebuffer is made in such a format (the shell draws
  into a known one and converts); a shell that doesn't know a mode
  refreshes as with `Full`.
- Plugin interface 6: the error enums have an `Other(u16)` variant (a
  cause a later build adds, by its code): `StorageError`, `HttpError`,
  `InstallFailure`, `PluginProblem`, `install::local::Refusal`,
  `DocumentError`, `ImageError`, `ZipError`, `BtError`, `DevError`,
  `RegisterError`, `FontError`, `CatalogError`, the plugin header's and
  the update file's `Problem`. HTTP and Bluetooth errors keep the code in
  event parameters (`other-7`).
- Plugin interface 6: kinds have an open value. `Other(&'static str)` (a
  name; a later build's values are constants of it) in `GestureKind`,
  `MenuPlace`, `library::catalog::Sort` and `Show`,
  `ActionHint`, `screen::Hint`, `canvas::Role`, `decorations::Tone` and
  `LineKind`; `Extended` in `SettingKind` (a `&'static str` key and static
  parameters: settings live in statics, and a key that needs no drop
  keeps that code out of every plugin) and `StatusValue` (key and
  `Params`). A build that doesn't know a value
  falls back: the default sort and filter, a gray tone, a solid line,
  a setting's value kept as text, a status value left out.
- Plugin interface 6: `BlockKind::Extended(u16)`: a block kind of a
  later build by its number (reserved: 1 a table's row, 2 a list item,
  3 a rule, 4 a figure's caption, 5 an aside, 6 a verse line, 7 maths),
  its content in the block's text, runs and annotations. A build that
  doesn't know it lays it out as a paragraph without a first-line indent
  (#33).
- Shared functions: family 7 is kept for the chain to families 8 and up
  (`paperpaw_table::FAMILIES`), so the plugin file's header, the
  firmware's table and the store's check keep their format while seven
  families suffice; `tools/shared-table.py` refuses an eighth.
- Plugin files: the header's reserved 16 bits (bytes 82 and 83) stay 0;
  a file that sets them is refused as one of a newer format ("needs a
  newer PaperPaw", in the store a plugin for another build) rather than
  as malformed.
- Plugin interface 6: `Align::Left` and `Align::Right` (`bits()` 5, 6):
  the page's edges whatever the paragraph's direction, as CSS
  `text-align: left`/`right`; `Start` and `End` stay the paragraph's.
  Layout resolves them by the paragraph's resolved direction (#33).
- Plugin interface 6: `WordBox::bidi` (`LineBidi`, also as
  `paperpaw_sdk::reading::LineBidi`): how the word's line goes, its
  paragraph's direction included, so a selection on a line of an Arabic
  book that holds only an English quote knows the line belongs to a
  right-to-left paragraph. The layout digest covers `WordBox`,
  `PageView`, `Page`, `PageLine`, `LineRun` and `LineBidi`.
- Plugin interface 6, second sweep of open values (#33): `Other(u16)` in
  `native::Chip` (a file for an unknown chip is still refused),
  `image::Format` (decoded as unsupported), `document::LinkKind`
  (followed as a plain link), `document::AnnotationKind` (shown as a
  gloss), `annotations::Kind` (by its file letter, an ASCII letter other
  than `b` and `h`: a mark of it is written with that letter, one with
  another left out; lines of a kind this build doesn't know stay
  as they are, as before) and `PluginKind`;
  and, as for the other errors, in `SettingError`, `data::Error`,
  `GraphicsError`, `config::ErrorKind` and `MemoryError`.
  `WritingMode` gains `SidewaysRl` and `SidewaysLr` (all of CSS Writing
  Modes 4).
- Plugin interface 6: `MenuNeed::Has(Capability)`: a menu entry can need
  any capability, also one a later build adds (not met where unknown).
  `SystemStatus` holds eight named values (`value`, `with_value`:
  `temperature-c`, `frontlight-percent`) a later shell can fill without a
  new interface.

### Added (plugin interface 6, for plugin authors)

- More freedom over how things look (ticket #57, interface 6):
  - **Text tone:** `ToneCurve::from_alphas`, `parse`, `parse_for` and `to_text` make a curve of your own from 16 values or text
    (`smooth`, `contrast:1`, `sharp:128`, a list of numbers); the settings `core.text-curve` (book text, in place of *Text
    contrast* when set) and `core.ui-text-curve` (the interface; themes set them) take a list of `[script=]curve` entries.
    A `FontHandle` carries a `TextInk` (`inked`, `text_ink`) that `draw` and `draw_bolder` use.
  - **Looks:** `Painters::text` paints the text of `Label`s and of the SDK's rows, headers, buttons, hints and captions (a shadow,
    an outline), never changing its width; `ui::draw_label_in(canvas, font, style, role, ..)` does the same for text a plugin
    draws itself (`draw_label` stays the stock look);
    `Painters::extras` takes a static list of painters for parts of kinds that come later (`PartKind`), and `Painters::flags` is
    reserved. `TextRole` is now an open `TextRole(u8)` with constants (`TextRole::BODY`, new `HINT`, `BUTTON`, `ICON`).
    **Breaking for plugin authors:** `TextRole::Body` is now `TextRole::BODY` (and so on for every role); rename it, the compiler
    finds each one.
  - **Script:** `Lang::likely_script()` (and `paperpaw_lang::likely_script_of(tag)`, which reads a tag without the language tables, so a plugin does not carry them) gives the script of a language (the tag's, else the one the language is usually
    written in: `ar`, `fa`, `ur` and `ps` are `Arab`, `he` is `Hebr`), so `arab=contrast:1` in the curve settings reaches an
    Arabic book and interface; `PageType.script` and `HookCx::script()` use it. Languages in Latin letters have none.
  - **Smaller plugins:** the kernel makes how the interface's letters are inked of `core.ui-text-curve` and the language
    (`ui::text_ink(value, language)`, `Fonts::set_ui_ink`), and the handles of the UI font carry it, so no plugin carries the
    parser (about 3 KB of each code plugin); `Fonts` gained a field for it.
  - **Rebuild all plugins:** `FontHandle` gained a field (its `TextInk`) that the firmware's `draw` reads, so every plugin has to
    be built against interface 6 again; one built before would pass a shorter handle.
  - **Colour roles:** `Ink` (the interface's colour roles) is now an open `Ink(u8)` with constants: `Ink::Text` is `Ink::TEXT`
    and so on (rename it; plugins only change the spelling). New roles `ACCENT`, `ON_ACCENT`, `LINK`, `SELECTION` and
    `WARNING` (numbers 14 to 18; 19 to 31 are free). `Palette` holds 32 colours (100 bytes, was 42) and knows which a theme set:
    a role it leaves out reads its `Ink::fallback()` (`LINK` reads `TEXT`, `SELECTION` reads `CARD`), so themes made for 14
    roles keep working and nothing looks different until a theme sets a new role. `core.ui-palette` also takes `#rrggbb`
    (older readers skip such a pair; gray panels use its brightness). `Ink` has `value`, `from_value`, `fallback`. `TextInk` is
    how text is inked (tone); an `Ink` is a colour role.
  - **Render hooks:** a plugin can register a hook (`Registrar::add_render_hook`, `RenderHookSpec`, `HookStage`, `BandView`,
    `HookCx`, `FramePass`, `Region`) that changes finished bands, with kernel-owned carry for dithering across bands.
    `RenderHookSpec` is `#[non_exhaustive]`: built with `new` and the `with_` methods. The
    kernel accepts hooks and does not run them yet: `Capability::RENDER_HOOK`, `RENDER_REGIONS` and `LOOK_EXTRAS` are off.
  - **Drawing:** `Canvas::pixel`, `gray_at` (read back what this band drew) and `fill_dithered`.
  - The interface check now follows type aliases to the types they name (a type only a `fn` alias mentioned was not pinned).

- `paperpaw-lang`: `Lang` (a language tag in 32 bits: language, script,
  region; canonical, so `per` and `fa` are one), `Direction` and
  `WritingMode` (#33). `paperpaw_sdk::lang`.
- `paperpaw-table`: the families of functions plugins may share with the
  firmware on RISC-V chips, their revisions, and `shared!` (#44);
  `tools/shared-table.py` writes its index from `shared.list`.
- Right-to-left text (#33): blocks of `paperpaw-document` carry a
  direction, a writing mode, a language, inline languages and a drop
  cap; links say what they are (`LinkKind`: a note reference, the way
  back). Lines of `paperpaw-layout` carry their direction (`LineBidi`),
  runs their level and language; pages their notes (`PageNote`);
  `Style` reserves `justify_cap`, options for some scripts as flag bits
  (`Style::script_options`, `ScriptOptions`: kashida, hanging
  punctuation, keep-all, and reserved groups for CJK and Thai line
  breaking, read and set with `group` and `set_group`), and options and
  signed values for later page settings (`LayoutOptions`,
  `Style::values`): 0 is today's behaviour, so a later option is a new
  bit, not a field; `BookStyles::from_bits`, and `ALL` holds styles named
  later; `Typesetter::advance_text` measures a text as a whole. The
  options are in the page-index key, so every saved page index is made
  again once after the update.
- `paperpaw-text`: `StoredPack` and `GlyphCache` are handles made by the
  firmware (`StoredPack::open`, `GlyphCache::new`), whose state only the
  firmware's code reads, so how packs and the cache keep it can change
  without a new plugin interface. Packs read from a card hold up to
  65,535 characters (all 11,172 Hangul syllables, #61), in far less
  memory: a block held whole takes some bytes, advances that are the same
  are kept as runs, and records are read 8,000 bytes at a time. A pack's
  index of characters takes at most 8 KiB on devices of a few hundred
  kilobytes, 32 KiB on larger ones (`PackLimits`, from the device's
  profile, so the emulator of the X4 refuses what the X4 refuses; else
  `FontError::TooLarge`), a size's advances at most 4 KiB or 16 KiB kept
  whole (else read when needed); `StoredPack::open_within` takes them,
  `open` the small ones. `FontError::OutOfMemory`: a pack that may open
  later.
  `StoredPack::render` returns `None` for a glyph it could not read now (a
  card pulled), which the cache does not keep; an advance that could not
  be read is 0 and counted (`StoredPack::failed_reads`, over the loaded
  fonts `Fonts::failed_reads`), so a page index can keep no pages
  measured meanwhile. `StoredPack::bytes` and `trim`,
  `GlyphCache::capacity`, `set_capacity` and `trim`.
- `paperpaw-text`: character sets are no longer public constants; tools
  name them (`charset::named`), so a set grows without a new plugin
  interface (`charset::names` lists them). The Greek and Cyrillic sets
  have the modifier apostrophe U+02BC (#60), which packs without it show
  as a right quote (`stand_in`); new sets `hangul`, `hangul-jamo`,
  `cjk-punctuation` and `fullwidth`. `paperpaw-fontgen --chars @FILE`
  reads code points from a file starting `# paperpaw-charset 1`, e.g.
  the 2,350 Hangul syllables of KS X 1001
  (`tools/fontgen/sets/hangul-ks.txt`).
- `paperpaw-text`: `Around` (what lies around a text, its base direction
  and language), `draw_text_in` and `measure_in`: right-to-left order,
  Arabic letters joined (also across a text's edges), mirrored brackets,
  marks kept on their letters.
- SDK: `WordBox::rtl`, `widgets::Align::Start` and `End`,
  `UiStyle::is_rtl`, `Env::direction` and `Env::lang`; the painters
  `sheet`, `cover`, `pill` and `ticks` (`SheetPart`, `CoverPart`,
  `PillPart`, `TicksPart`); room to grow without a new interface
  (`Widget::on_extended`, `GestureRecognizer::extended`,
  `library::Library::extended`, `Document::extended`).
- Reading Arabic and Hebrew books (#33): EPUB blocks take their
  direction and language from `dir`, `lang`/`xml:lang` and CSS
  `direction`, and a book in a right-to-left language runs right to left;
  `text-align: left`/`right` and the margins at the sides are read
  against the direction; inline `dir`, `<bdi>` and `<bdo>` set their
  text apart (isolates and overrides of UAX #9). Lines are resolved by
  UAX #9, start at the right where they run right to left, and are drawn
  run by run in the order of their levels, joined Arabic letters as wide
  as drawn (`FontHandle::measure_in`, `draw_in`), a word whose
  characters come from more than one font (in quotes or brackets, with a
  colon or digits) in parts of one font each; the words of a page
  come in reading order where they are drawn. Not yet checked by readers
  of these languages.
- The pieces of a long paragraph (a chapter saved by Word as one) are
  resolved as one paragraph: `paperpaw_layout::PieceStarts` keeps the
  bidi state at their starts for a reader (`LayoutContext::pieces`), and
  `paperpaw_text::bidi` gives that state as bytes (`state_len`,
  `start_state`, `resolve_from`, `skip_from`, `state_paragraph`), which
  stay out of the plugin interface. `paperpaw_text::bidi` is public
  (levels, paragraph levels, rules L1 and L2).
- `paperpaw_text::is_invisible` (also `paperpaw_sdk::text`): the controls
  that draw nothing and take no room.
- `Block::continued`: the next block continues this one (the pieces of a
  long paragraph, set by the EPUB and TXT documents), so the layout reads
  ahead only then. `Lang::direction`: the direction a language is
  written in.
- `paperpaw-i18n`: a catalog says its direction, script, digits and
  calendar (`[meta]`); Arabic and Hebrew plural rules.
- Arabic harakat and Hebrew points sit where their font puts them (#33):
  `build_pack` (and `paperpaw-fontgen`) keeps the GPOS `mark` and `mkmk`
  anchors of Arabic and Hebrew in a pack's anchors table (for each
  letter's form, under or over its dots), and `draw_text_in` draws a mark
  on its letter, or on the mark before it, by them; a mark whose classes
  its letter lacks is centred over it. Packs read a glyph at a time and
  outline fonts keep the anchors of at most 48 keys in the glyph cache
  (about 2.5 KB, once marks are drawn so). Packs without the table, and
  every Latin, Cyrillic and Greek pack, are drawn as before. The Arabic
  and Hebrew data plugins (`tools/script-pack.py`) are about 12 % larger.
  `tools/script-pack.py` without a set builds the plugins
  `tools/plugin-files/data.toml` lists for it (else Cyrillic and Greek,
  as before; its folders keep their names, `script-<sets>`). Of two sizes of a pack
  as near to the size asked, the smaller is drawn, whatever their order
  in the pack.
- Plugins switch the device off where PaperPaw does so
  (`Command::PowerOff`, `Capabilities::power_off`) and hear how long it
  slept (`Env::slept_for`) (#51).
- The X4 and X3 share the **text family** with their code plugins (43
  functions of `paperpaw_text` and the SDK's font handles, revision 1):
  a plugin built for them calls the firmware's copy of measuring, drawing
  and wrapping text instead of carrying its own (about 26 KiB less in a
  plugin that draws text). The loader fills the families' words of a
  plugin (`Relocator::tables`), refuses a plugin built for a newer or an
  older revision of a family before it places anything, `paperpaw-sign
  pack` records the families a plugin reads, and a panic in shared code
  (a parser of what the plugin handed it, an allocation that failed
  there) is blamed on the plugin that called it, so safe mode switches
  off that plugin alone; the crash file says `[shared code, called from
  <plugin>]`.
  The X4 Pro and the X4 Classic keep their copies. A library built for a
  computer with `--cfg paperpaw_table_host` calls the same tables; CI job
  `table-compat` checks old plugins against the new firmware once an
  interface has been raised with tables.

### Fixed

- The device no longer falls asleep while the library is being walked (#105): a first walk of a few thousand books
  takes longer than "Sleep after", and sleep stopped it. The sleep is put off while the walk goes on, for an hour
  after your last press at most, and not at all with the battery at 10 % or less (not charging); "Switch off after"
  counts from the sleep, so it waits too. A walk paused for an open book holds nothing. The walk keeps the device
  awake even with "Plugins may keep awake" switched off (the help of that setting says so).
- X4 and X3: books copied to the card while the reader slept lightly (the card in a computer) show in the library
  again after the wake (#79, #373), also when the library or a book from it was open at the sleep, and a look at
  the card that was under way at the sleep is started again. A wake is no start, so the library had been left as it was.
- The store no longer counts code plugins that are switched off as taking room (#70): the next start skips their
  code and their room is free, so an install that fits then is offered (its page says it fits while the code
  plugins you switched off stay off), instead of asking to remove a plugin that could have been switched off.
  The update does the same when it fetches code plugins for a new version.
- The reader no longer keeps pages counted while a font pack on the card could not be read (a card pulled, a
  hiccup): the letters it could not measure were 0 wide, and a page index of them sent later pages to the
  wrong place. Such a step is made again (`Fonts::failed_reads` is taken before and after it); after three in a
  row the count is given up for that layout, as for a page that cannot be laid out.
- A pack that adds another script's letters in more sizes than the font
  it extends has (the Arabic one, to the firmware's Literata) no longer
  becomes that font's pack of larger sizes: the font's largest size
  stays its own, and Latin text at larger sizes is not set in it.
- `decorations::join_boxes`, and the reader's selection, join the boxes
  of words next to each other in a line that runs right to left (they
  came to nothing) and leave out a word drawn between two of a line that
  runs both ways; left and right move the selection to the word drawn
  beside.
- SDK: `layout::line_start` of a position before a block (a section's start,
  say) gives that block's start, where a page laid out from there starts, not
  the position itself. Comparing it with a page's start or looking it up in a
  `PageIndex` then needs no more care.
- Choosing text with the buttons: the page buttons (the X4's side buttons) now move by line, as Up and Down do,
  not word by word, and Left from the first word of the page reaches the last word (Right from the last
  reaches the first), instead of stopping there.
- *Page with a picture*: a picture made for the whole screen (the X4's 480 × 800, as CrossPoint's) now covers the
  whole screen at size 100 %. The size was a share of the screen's shorter side, so it was drawn in a square
  and covered only the middle; it is now a share of the screen, and at 100 % no margin is left. Pictures
  that are wider than high keep their size.
- A book read to its last page is now recorded as read to 100 % (it was recorded as how far the start of its last page
  is, often 95 %, so the library did not list it as finished). The reader's `core.book-finished` and
  `core.book-closed` now carry 100 once the last page was reached.
- A time zone rule with an offset of more than 24 hours (such as `<>200`) is now refused instead of read as an offset of days.
- Reader: going to a chapter whose table of contents entry names no place in
  it (a whole chapter of an EPUB) showed the last page of the chapter before,
  from the contents, a link and *Next chapter* alike, and *Next chapter* and
  *Previous chapter* could go no further from there.
- Reader: status lines show the page in the chapter in a book's first chapter
  too, where it starts at the book's start.

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
