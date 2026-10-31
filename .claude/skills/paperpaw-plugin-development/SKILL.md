---
name: paperpaw-plugin-development
description: "Helps write a plugin for PaperPaw as it is, without changing PaperPaw (a community plugin, usually in its own repository): code plugins in Rust against the released paperpaw-sdk and data plugins without code; trying them in the desktop emulator and on a real Xteink X4; measuring speed, heap and stack; keeping within the memory of small devices; publishing it in the plugin store on paperpaw.org (the upload form or a tagged GitHub release, signing code plugins, reproducible builds, updates). Use when someone writes, ports, debugs, tests, profiles or publishes such a plugin. Not for work on PaperPaw itself (its kernel, SDK, firmware or its own plugins in plugins/), where extending the SDK is the rule: that follows CONTRIBUTING.md."
---

# Developing PaperPaw plugins

PaperPaw is an e-reader system in Rust: a small `no_std` kernel hosts
plugins, and everything a reader sees comes from plugins (home screen,
reader, library, menus, settings, keyboard, formats). A plugin can do
anything the built-in ones do and replace any of them.

PaperPaw changes quickly. This skill says how to work and where the
truth is; it names no versions. **Before relying on any name, flag,
number or path below, check it against the repository** (the docs, the
SDK's API, `--help` of the tools). Where they disagree, the repository
wins; tell the user if this skill looks out of date.

## Who this is for

Authors of plugins for PaperPaw as readers have it, who do not change
PaperPaw. Working on PaperPaw itself (its kernel, SDK, firmware or its
own plugins in `plugins/`) is different: there, a plugin that cannot do
something well is a reason to extend the SDK, and `CONTRIBUTING.md` is
the guide. The limits below apply only to plugin authors.

## 1. Find the current state first

Read, in this order, and skim what is relevant to the task:

1. `docs/plugins.md`: how plugins are written, what they contribute, the
   rules for screens, memory, testing, data plugins.
2. `CONTRIBUTING.md`: the architecture rules reviews check, and the
   checks CI runs.
3. `samples/hello-plugin`: a complete small plugin with a test; the best
   template. `samples/hello-native` builds it as a plugin file for the
   X4s, in a workspace of its own as a plugin's repository is, with a CI
   job. `samples/plugins` holds data plugins.
4. The SDK's API: `cargo doc -p paperpaw-sdk --open`, and
   `crates/paperpaw-sdk/api.txt` (every public item, kept current).
5. `CHANGELOG.md`, its top section: what changed lately (renamed or
   removed API is noted there).
6. `docs/adr/`: why things are as they are (sans-IO kernel, everything a
   plugin, banded rendering, streaming conversion, fonts, network).
7. `plugins/*`: the first-party plugins are ordinary plugins. Find one
   that does something similar and follow its patterns.

## 2. The model in brief

- **Plugin**: a type implementing `Plugin`: a manifest and `register`,
  which adds contributions through a `Registrar` once at start (actions,
  screens, slots, menu entries, settings, event handlers, key maps,
  formats, fonts, widgets, status items, services, translations, …;
  the table in `docs/plugins.md` lists them).
- **Read through `Env`, change through `Command`s.** Plugins never change
  the kernel's state directly; the kernel runs the commands after the
  plugin's code returns. This keeps plugins testable. Read `Env`
  through its methods (`env.settings()`, `env.i18n()`, `env.device()`,
  `env.storage()`, `env.caps()`, `env.services()`, `env.now()`, one for
  each part), never through its fields: the fields are being made
  private so that `Env` can carry more without a change to plugins.
- **Handlers queue their commands in a `Commands`**, never a
  `&mut Vec<Command>`: an action's `.handler(|env, params, out| ..)`,
  an event's `r.on(EVENT, |env, data, out| ..)`, work steps. `out` has
  `push`, `extend` and `emit` and reads as a slice. An action always has
  params (empty from a button or a gesture): `Command::run(id)` runs
  one without, `Command::RunAction(id, params)` with, and a screen reads
  them with `cx.params()` in `on_action`. Commands come with builders
  where they are many-fielded: `Command::set(&SETTING, value)`,
  `Command::http(HttpRequest::get(url, &REPLY))`.
- **Settings, services and answers are typed.** A setting is a
  `Setting<bool | i64 | Text>` constant, its spec an entry of a `static`
  list built with the `const` constructors of `SettingSpec` (`switch`,
  `number`, `choice`, `text`, `language`, `list`; what few need goes
  into a `static SettingExtra`, taken with `with_extra`), registered
  with `r.add_settings(&SETTINGS)`; groups are `static SettingGroupSpec`s.
  A service type names its contract with `service_type!(Type, "plugin.name/1")`
  (its key changes when its fields do), is provided with
  `r.provide(id, value, version)` and found with
  `env.services().get::<Type>(id, min_version)`; wrap a trait object in
  a struct of your own. Answers come as typed events: a plugin names the
  `Event<HttpReply>` (or `Typed`, `Picked`, `Confirmed` of `ask::*`) it
  wants and passes it with the request; the answer's fields are read
  from the event's data, never from strings.
- **Screens** draw, react to actions (buttons, gestures bound to actions),
  gestures and events. **Slots** are roles (home, reader, library, menu,
  …) that screens fill: binding to a slot is how a plugin replaces
  another. **Events** tell plugins what happened.
- **Ids are namespaced** (`myplugin.thing`); `core.*` is the kernel's.
  Texts are translation keys; every plugin brings catalogs (English
  first) and a test that all languages have the same keys.
- **Data plugins**: a folder with `plugin.ini` and files (translations,
  key maps, setting defaults, fonts, font packs, stylesheets). No code,
  they work on every device as they are.
- **Plugins build on the SDK as it is.** They depend on `paperpaw-sdk`
  only, never on the kernel crate, and must run on PaperPaw as readers
  have it: never on a patched kernel or SDK.
- **When something seems impossible**, look for the existing way first:
  bind a slot to replace a screen, use a service or contribution point
  another plugin offers, an input, text or refresh filter, an event, a
  setting default. Many first-party plugins solve similar problems;
  copy their approach. If the SDK really lacks something, do what fits
  today in the plugin, and suggest the addition to the PaperPaw project
  (an issue saying what the plugin needs and why); changing PaperPaw
  itself is a contribution to it, under its own rules
  (`CONTRIBUTING.md`), not part of writing a plugin.

- **Screens are drawn with the SDK's `ui` building blocks**, never with
  positions computed by hand. In `draw`, make a `ui::Ui` from the canvas,
  the environment and the style (`let mut ui = ui::Ui::new(canvas, env,
  &self.style)`): its methods draw the parts (`ui.card(..)`,
  `ui.button(..)`, `ui.header(..)`, `ui.message(..)`, `ui.empty(..)`,
  `ui.focus(..)`), and the other blocks take `&mut ui`: `ui::Header`,
  `ui::ListView` with `ui::Row` (`Row::value`, `Row::plain`,
  `Row::switch`, `Row::number`, each one value that gives `height`,
  `draw` and `hit`, so they cannot disagree), `ui::Label` for text
  (`Label::new(text).role(TextRole::SMALL).align(..).wrap()`),
  `ui::Segments` and `ui::Chips` (given their labels once, when made),
  `ui::Dialog`, `ui::TextView`, `ui::DatePicker` and `ui::TimePicker`
  (with `time::Date` and `time::Time`). Layout comes from `ui::Stack`,
  `ui::columns` and `ui::CellGrid`; asking the reader from `ask::text`,
  `ask::choice`, `ask::confirm`. Sizes are read from `UiStyle::new(env)`
  with its methods (`style.margin()`, `gap()`, `row()`, `text()`,
  `small()`), so the screen follows the reader's text size and look and
  works by touch and with buttons alone. The building blocks that take
  `on_action` and `on_gesture` answer with a `ui::Response` (`Ignored`
  is the screen's, `Same`, `Changed` to draw again, `Done` with a
  result): handle `Ignored` yourself. Show a focus only where
  `ui::focus_shown(env)` says so. The table
  "Which building block for what" in `docs/plugins.md` says which for
  what; `paperpaw-dev ui-kit` shows them all. Use those the SDK release
  you build against has, and look up a name in `api.txt` before using
  it.

## 3. Rules that matter on small devices

The smallest supported device (the Xteink X4, an ESP32-C3) has a few
hundred KiB of RAM in all, no FPU, and draws frames in bands. Look up
the current heap budget in the emulator's X4 profile and in the memory
section of `docs/plugins.md`. A plugin that should run there:

- **Keeps little in memory**: lists in files read as needed; never a
  whole book or large file in memory; reads with `read_at` a piece at a
  time; writes downloads straight to storage.
- **Allocates fallibly** where sizes come from data (`try_reserve`) and
  copes when that fails; checks the device's memory class to do less.
- **Draws cheaply.** `draw` runs once for every band of a frame and must
  not change state. Read and compute in the screen's own state (when it
  opens, on events, on a timer), then draw from that. Skip what lies
  outside `canvas.clip()`. Widgets read what they show in `update`,
  never in `draw`.
- **Looks files up rarely.** On a memory card each file looked up is a
  search through its folder, tens of milliseconds on the X4: take what a
  listing already told (sizes), keep small copies of pictures shown in
  lists, and decode big pictures when they show, not ahead.
- **Asks for small redraws**: the area that changed, not the whole
  screen, and the lightest refresh that looks right.
- **Watches the stack**, which is small and not growable. No large
  arrays or structs as locals or by value (build big state on the heap in
  place, e.g. `vec.resize_with(1, T::default)` rather than
  `Box::new(T::new())`, which may go over the stack first); no deep
  recursion over data from books. Rust's stable `sort` keeps a scratch
  buffer of a few KB on the stack; prefer `sort_unstable*` where order of
  equals does not matter.
- **Keeps big constant tables free of pointers** (no tables of `&str`):
  on the X4 they can end up partly in RAM.
- **Works by touch and by buttons alike.** Some devices have only
  buttons. Buttons move a focus and confirm; taps act at once; the focus
  shows only once buttons are used.
- **Never panics on bad input**: books, configuration and files can be
  broken; warn and carry on.
- **Keeps its files in its own folder** (`Env::plugin_dir`), and says
  which version of its data files it writes, so later versions can read
  them.

## 4. Build and test

- Hold the plugin to what PaperPaw holds its own code to
  (`CONTRIBUTING.md` lists those checks): format, clippy with warnings
  as errors, tests, and a build for the embedded target (`no_std`), so it
  runs on the smallest device too. Run them before saying something is
  done.
- Follow PaperPaw's code guidelines (`CONTRIBUTING.md`, *Code
  guidelines*): length limits, errors on user data never dropped,
  memory, and comments in plain English.
- Build against a released SDK version. The manifest names no SDK
  (`Manifest::new(id, name, version)`): a plugin with code is loaded by
  its file's fingerprint, a data plugin by its `formats`. When moving
  to a newer SDK, read the CHANGELOG for what changed. Until PaperPaw
  1.0, a plugin file (`.ppn`) runs only on the release it was built for
  (its fingerprint holds the SDK's version and the compiler): build it
  again, and publish it again, for every release.
- Give the plugin a version PaperPaw can read: `Version::new(1, 2, 0)`,
  or `package_version!()` for the crate's own. It is the plugin's own,
  apart from PaperPaw's (PaperPaw's own plugins have theirs too): raise
  it when the plugin changes, as devices update only to a newer one. A pre-release is
  `alpha`, `beta`, `dev`, `pre` or `rc` with an optional number up to
  8190 (`0.5.0-beta.2`), ordered as SemVer orders them, so a later beta
  is an update. Any other pre-release (`-gamma`, `-beta.01`) is no
  version: a data plugin with one is refused with that message,
  `paperpaw-dev package` says so (`package.version`), and a crate's
  `package_version!()` stops the build.
- Test plugins with the real kernel and in-memory storage, as the sample
  does: build a kernel with the plugin, send input, look at the screen
  shown, the settings, the files, the network requests, and answer them.
- Screens can be checked as text (what a screen describes, part by part)
  and as pictures in the emulator; the repository's snapshot tests show
  how, and say how to update a snapshot when a change is meant.
- Write a flow for every action of the plugin, as the main test of what
  it does: each thing a user does with it (a setting switched, an entry
  made, a sync started), from where they start to what should be true
  after, by touch and with buttons alone (`docs/plugins.md`, "Flows";
  `paperpaw-emu --record` writes the steps). Keep them in `tests/flows`
  of the plugin's repository and run `paperpaw-dev flows --flow
  tests/flows` (the templates in `tools/dev/templates` set it up, CI
  included): every step's heap and frame time is held against the X4's
  budgets with the step named, and each flow is played again at the
  edges (steps repeated, *Back*, *Home* and sleep in the middle, the
  network failing, slow, empty or huge, the pseudo-language, long data,
  little storage). Fix what a variation finds rather than leaving the
  flow out; `paperpaw-dev explore` is only a cheap baseline beside them.
- Flows and their pictures stay out of the plugin's package (a data
  plugin's folder): `paperpaw-dev package` warns of any it finds.
- Say how the plugin works: `how/en.txt` (and one per other language),
  named by the manifest with `Manifest::how` (`docs/plugins.md`,
  Translations), a few sentences on what it does, where it is found and
  how it is used
  (the plugin manager and the store show it; `lint.how-it-works` warns
  where it is missing or short). Give its screens first-time hints with
  `help::add` (`docs/plugins.md`, "Hints for a screen").

## 5. The desktop emulator

`shells/desktop`, run with `cargo run -p paperpaw-emulator -- …`. Check
`--help` for the current options; typically:

- `--list-profiles`, `--profile <id>`: emulate a device (screen, input,
  memory budget). Try at least the smallest device and a touch device.
- A window by default (keys stand for buttons, the mouse for touch), or
  `--headless --script "<steps>" --screenshot <file>` for scripted runs:
  steps such as button names, `long:<button>`, `wait:<ms>`, taps,
  `shot:<file>` (relative paths; a drive letter's colon reads as a step
  separator).
- `--library <dir>`: the folder that stands for the device's storage
  (the emulator writes into it: give it a copy, so tests start fresh); `--set key=value` and `--config` for
  settings.
- Memory: the profile's heap budget is enforced; the run ends with the
  heap now and its peak. `--heap-budget none` measures beyond the limit;
  `--heap-report` lists the live heap by call site; `--trace-alloc
  <bytes>` prints every allocation of at least that size with where it
  comes from (use a build with debug info for names).
- Plugins with code are compiled into the emulator: in a local checkout,
  add yours as the sample is added (a Cargo feature of the emulator), to
  try it. Data plugins need no build: copy the folder into the storage
  folder's `plugins`.
- `paperpaw-dev` (`tools/dev`, `docs/plugins.md` "Checking your plugin")
  runs the same device model on every profile and checks what the
  plugin adds; `shells/web` is the same in a browser
  (`docs/web-emulator.md`).

## 6. A real Xteink X4

Only if the user has one and wants it used. Its README is
`shells/xteink/README.md`; read it first.

- **Save the stock firmware first** (the README says how), so the device
  can always go back. Flashing replaces the firmware.
- Build and flash from `shells/xteink` (its own workspace); a test
  build has the remote control below, released firmware does not.
- A plugin with code is tried as a file, without building it into the
  firmware: build it as `samples/hello-native` does (`cargo x4`, or
  `cargo +esp x4pro` for the X4 Pro and the X4 Classic, with the
  release's core libraries installed into the `esp` toolchain, see
  `tools/xtensa-std`) against the SDK of the firmware on the device, pack it with `paperpaw-sign pack`, put
  its folder with the file as `code.ppn` onto the card and name it as
  installed (`python tools/x4-remote.py --push <folder>
  plugins/<id>`, then `--trust-code <id>`), and restart. The plugin
  manager says why a file does not run (another fingerprint, damaged,
  no room); `docs/plugins.md`, "Plugins as files", has the details.
- `tools/x4-remote.py` drives a test build over USB (see its docstring
  and `--help`): presses buttons, runs actions, opens books, prints the
  screen's id and what it shows, takes screenshots, puts files onto the
  card and gets them back (also whole folders, checked by MD5), lists,
  moves and removes files, shows the log (`-v`, its level with `log:`),
  the heap and the stack left (`heap`), what each plugin took (`usage`),
  a sampling profile (`--profile`) and the storage calls (`--trace`). It
  waits for and checks what the screen shows (`until:`, `expect:`, an
  exit code for scripts), taps and swipes, sends events (`emit:`), sets
  the clock, and restarts the X4 or makes it panic, waiting until it is
  back. Released firmware has no remote control.
- **Keep the device reachable.** Never start anything that could cut the
  USB link (sleep, long blocking loops) without a bounded timeout after
  which it comes back by itself; run tools with timeouts. Ask before
  flashing or anything the user would have to undo by hand.
- After a crash the X4 starts again, often reopening the last book, which
  can look like success. Check the log for panics and the crash file the
  firmware keeps on the card (its path is in the firmware's crash
  module), and that what you opened is really what shows.

## 7. Measuring speed and memory

Measure before and after a change, the same way, on the same data, and
report the numbers; never claim a speed-up from reading code alone.

- **Frames**: with the log on, each frame reports its area, the time to
  draw it and the time the panel took. Drawing is what code can change;
  the panel's refresh is mostly fixed by the hardware.
- **Where time goes**: the X4 profiler names the functions the processor
  was in most; storage (card and file system) often dominates, then
  decoding (pictures) and text.
- **Storage**: the trace counts calls, bytes and card blocks; repeated
  blocks mean a cache is missed.
- **Heap**: the emulator's peak and call sites; on the X4 `heap` reports
  used, free and the largest free block (fragmentation: a large
  allocation can fail with plenty free in pieces).
- **Stack**: `heap` reports the stack never used since start, but it
  misses frames that reserve stack without writing it. To find big
  frames, disassemble the release firmware (llvm-objdump from rustup's
  llvm-tools) and read each function's prologue (`addi sp, sp, -N`, or
  `lui` then `sub sp` for large frames). Keep a clear margin after the
  deepest work (e.g. a first opening of a book with a large cover).
- **RAM layout**: compare the firmware's sections between builds
  (llvm-objdump `-h`): more `.data` or `.bss` leaves less stack.

## 8. Working cleanly

- Keep the plugin self-contained: its own ids, its files in its own
  folder, its own translations. Use other plugins only through what they
  publish (services, settings, events, contribution points), never
  through their files or internals.
- Write it as PaperPaw's own plugins are written: the SDK's types and
  helpers rather than home-made ones, the same idioms for screens,
  lists, settings and translations, so it looks and behaves like the
  rest and keeps working as the SDK grows.
- Prefer replacing or adding to a part through the SDK (a slot, a menu
  entry, a filter) over copying a first-party plugin and changing it.
- Small, focused commits with messages that say why; tests and the
  plugin's own documentation with the change; its version raised when
  it is released.
- Check both input modes, at least two languages, the smallest device's
  memory, and how it looks in the emulator.
- When the user must decide (what the plugin should do, anything hard
  to undo), lay out the options with a recommendation and ask.

## 9. Publishing

When the user wants the plugin in the store (on paperpaw.org and in
every device's *More plugins*), follow
[`publishing.md`](publishing.md) in this folder, step by step, with the
site's guide for authors (<https://paperpaw.org/authors>) as the
reference for the site's names: what to upload (the folder with
`store/store.ini`, a code plugin's signed builds and source, or all of
it in one store file, as the PaperPaw Workshop makes it), the
author's signing key, builds the site can rebuild to compare, the
upload form, or instead a tagged release on GitHub that the site takes
(unreviewed, section 6b), updates, every check code with
its fix, and a final checklist. Its rules for the
assistant come first: the secret key is never shown, copied or
committed, nothing is uploaded, pushed or submitted without the user's
yes, and no field or address is invented.
