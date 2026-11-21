# 0030. Screens that run in real time

- Status: Accepted
- Date: 2026-10-25

## Context

PaperPaw's SDK was made for pages: a screen draws when something
changes, buttons arrive as actions once let go, refreshes are chosen by
the kernel, and the X4's main loop waits while the panel refreshes.
Plugins that run in real time (an emulator, a game, an animation, a
drawing that follows the pen) need more, and before this they worked
around the SDK:

- **Buttons** came only when let go (`Screen::on_button`), or through an
  input filter, which sees every screen's input.
- **Drawing a picture** went pixel by pixel through `Canvas`, each
  dithered and encoded with a division.
- **Code** of a plugin runs from flash through the cache on the X4
  ([ADR 0024](0024-native-code-plugins.md)); a tight loop that misses
  the cache is several times slower than from internal RAM.
- **The screen** came with the row of hints, full refreshes every few
  pages, gray refreshes for pictures, the auto-sleep of a reader, and on
  the X4 a main loop that ticked no screen while the panel refreshed.
- **Memory** came from wherever the heap had room: on the X4 Pro PSRAM
  first, also for data a loop reads all the time.

Panels and their drivers will get faster (custom waveforms, overlapped
partial refreshes, faster SPI). Plugins written now must gain from that
without being rebuilt, and the SDK must not be redone for it.

## Decision

- **A screen says how it runs, in one place.** `Screen::mode` returns a
  `ScreenMode`, read whenever it counts (each input, each frame; whether
  it is full screen when the screen is made). It asks for intent, not
  for a device's modes:
  - `full_screen`: no row of hints, no bars; the screen has the panel.
  - `raw_keys`: buttons come pressed and released (`Screen::on_key`),
    with the time; `Env::keys_held` says which are down now.
  - `cleans_itself`: the kernel adds no full refresh every few pages
    and no gray refresh for pictures; the screen asks for one when it
    wants it (`Cx::request_refresh`).
  - `fastest`: changes are shown with the fastest refresh the panel has,
    quality second.
  - `grays(n)`: `n` grays are enough (2: black and white).
  - `frame_every(Duration)`: the pace it would like. The device keeps
    ready for it (no light sleep longer than a frame) and shows frames
    as soon as the panel can take them.
  - `awake_for(Duration)`: the device sleeps by itself no sooner while
    the screen shows, as long as the user's setting is shorter.
  - `memory(bytes)` (or `memory_in(placement, bytes)`), `needs_network`,
    `needs_bluetooth`: the room it needs free, and the radios it needs
    (see the lease below).
- **The device says what it can do, and what it did.** `Env::panel`
  returns a `PanelInfo`: the fastest time of each refresh mode as
  measured (`refresh_time`), its grays, whether the next frame can be
  drawn while one refreshes (`overlaps`), and the last frame
  (`FrameReport`: its area, its mode, how long it took to draw and to
  show, when it was shown, and how many frames were merged into it).
  A plugin paces itself by that, not by numbers of its own.
- **Raw keys keep a way out.** The power button is never raw: sleep
  stays. Holding *Back* for two seconds (`ESCAPE_HOLD`) leaves the
  screen whatever it does with *Back* otherwise. On touch screens a
  finger held as long in the top left corner does the same for a screen
  that is full screen or takes raw keys, and the gestures from the
  edges stay unless the screen holds them (`Screen::holds_edges`). A press the screen does not take
  (`Handled::No` when pressed) goes to the key map with its release, as
  before; one it takes, it gets let go too.
- **A blit draws packed pictures fast.** `Canvas::blit` draws a
  `Bitmap` (1, 2, 4 or 8 bits a pixel, packed, an optional palette of
  grays) at an integer scale (`Blit`), dithered or not. Values for each
  palette entry are worked out once a call; rows are built in words for
  the 1, 2 and 4 bit framebuffers. A frame of 160×144 at three times
  its size is a fraction of the cost of drawing it pixel by pixel.
- **Hot code runs from internal RAM where the device has it.** A plugin
  marks functions with `paperpaw_sdk::hot!`, which puts them into
  `.text.paperpaw_hot`; the linker script keeps that section at the end
  of `.text`, so a file whose hot code stays in place runs as before.
  The packer records the range (`Header::hot_at`, `hot_len`) and every
  reference across its edge (a call or a load of an address relative to
  the instruction), which it can patch: `auipc`/`jalr` and
  `auipc`/`addi`/`lw`/`sw` pairs on RISC-V. References it cannot patch
  across the edge (a jump of limited reach, a difference of addresses)
  fail the pack, saying which function to mark or not. Plugins link
  without relaxation (`--no-relax`), so calls stay patchable. The X4
  copies the hot code into an arena of internal RAM whose place is the
  same at every start of a firmware (a static, whatever no plugin takes
  given to the heap), so the cold code placed once in flash keeps
  calling it; the index of the plugins in flash records each one's place
  in the arena. The arena is bounded (`HOT_ARENA`), each plugin's part
  too (`HOT_MAX`): what does not fit runs from flash, as before. The X4
  Pro keeps hot code in PSRAM for now: its calls cannot reach from
  internal RAM to PSRAM without trampolines, and PSRAM through its cache
  is already fast; the format leaves room to do it later.
- **Data goes where it is used.** `memory::Placement` (`FAST` for data
  a loop reads all the time, `LARGE` for much data read seldom) and
  `memory::vec_in` allocate in internal RAM or PSRAM on the X4 Pro, and
  anywhere where a device has one kind of memory. The firmware gives
  code plugins the call through its table (`Api::alloc_in`), which grows
  at its end.
- **Room is lent to a screen that needs memory.** As such a screen
  shows, the kernel makes room step by step until there is enough,
  measuring after each: caches let go (glyphs drawn, files read), the
  background work held back, Wi-Fi and Bluetooth off where the screen
  does not need them (never while a download goes on, a phone is
  served, or a Bluetooth link is up), the screens below closed but the
  lowest (each keeps what it has as it closes, a book its place). Where
  there are internal RAM and PSRAM, the room is measured where the
  screen asked (`Placement::FAST`, `LARGE`). The screen reads what it got
  (`Env::room`: granted, partly or refused, with the memory free and the
  steps taken, `RoomSteps`) and makes its buffers in `on_show`. Once it
  closes everything comes back: the screens opened again, Wi-Fi as it
  was (the network wanted again if it was in one), Bluetooth let go,
  the work going on. Nothing is said to the reader. Steps are the
  kernel's: new kinds of room are new `RoomSteps` constants.
- **Plugins may hold the device awake, within bounds.** A screen
  (`Cx::keep_awake`), work, a service or a widget (`Command::KeepAwake`
  with a `power::WakeHold`) keeps the device from going to sleep by
  itself: each hold has an id (the same id renews it), a reason, and an
  end at most `MAX_HOLD` away; a screen's counts while it is on top and
  ends when it closes. Holds put off only the sleep after
  `core.sleep-after`: the processor still naps, the power button still
  works, and below 10 % of charge (not charging) no hold counts. The
  reader could let no plugin hold it (`core.awake-holds`, on by default;
  not offered in interface 7, which carries no wake holds, #459);
  `PluginUsage` counts each plugin's awake time and its last reason.
  `ScreenMode::awake_for` is one use of it.
- **The X4's main loop goes on while the panel refreshes**, while a
  screen runs at a pace: it starts the refresh and returns; screens get
  their timers and the buttons are read; a frame drawn meanwhile waits
  until the panel is free and then shows what is current. Other screens
  keep the loop as it was. What goes when is decided by a small state
  machine (`paperpaw_x4::paced::Frames`), tested to never send to the
  panel while it refreshes and to lose no frame, also when the device
  is about to sleep.
- **Growing without a new interface.** `ScreenMode` and `PanelInfo` are
  values of fixed layout with private fields: a word of flags and slots
  of numbers, more than are used. A later option takes a free bit or
  slot: a plugin built before leaves it zero, which means as before; a
  firmware built before ignores what it does not know. So new modes of
  running, refresh times of new modes, more grays, overlapped or
  pipelined refreshes and faster panels come as kernel and device
  changes behind the same calls. Bitmap formats, dithers and places of
  memory are constants of one type each (`BitmapFormat::GRAY2`,
  `Dither::ORDERED`, `Placement::FAST`), not enums: a new one is a new
  constant, which the interface gate counts as free. A new button is a
  new name; touch and pen are `Screen::on_pointer` already. For what
  cannot be foreseen, interface 4 has keyed escape hatches:
  `Command::Extended`, `Screen::on_extended` and `Contribution::Extended`
  (a kernel leaves keys it does not know), and `Capabilities::has` with
  `Capability` constants. `Moments` and `BookStyles` were widened, and
  `Storage` got `write_at` and `truncate`, so that these need no raise
  either. What needs a new
  interface: a new item of `Screen` or of another trait, a field of a
  type whose layout crosses, a new relocation kind for hot code (the
  file's format names it), and anything gone.

## Consequences

- An emulator or a game is a plugin like any other, built against the
  SDK as released, with no input filter, no drawing by pixel and no
  copy of the kernel's refresh rules.
- Plugins that keep the default mode see nothing new.
- Hot code costs heap on the X4: the arena's part a plugin takes is
  memory the kernel does not have; it is counted in the plugin's load
  log and bounded.
- The X4's loop has two ways of showing a frame, waiting and not; the
  second is used only while a screen asks for a pace, so the reading
  path stays the one tested on devices.
- Interface 4 carries all of it (one raise for beta.8).
