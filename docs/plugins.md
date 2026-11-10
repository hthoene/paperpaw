# Writing plugins

Everything PaperPaw shows and does comes from plugins: the home screen,
the reader, the library, the menus, the settings, even the keyboard. The
kernel only hosts them. A plugin you write can do anything these do, and
replace any of them.

There are two kinds:

- **Data plugins**: a folder with a `plugin.ini` and data files, no code.
  They bring translations (also whole new languages), key maps, new
  defaults for any setting, presets (themes, pictures for the sleep
  screen), menus and actions made of steps, marks in the text,
  stylesheets and fonts.
  They work on every device as they are; see
  [Plugins without code](#plugins-without-code).
- **Code plugins**: Rust crates against the SDK (`paperpaw-sdk`). They
  bring screens, actions, settings, formats, widgets, services, anything.
  They are compiled into a build of PaperPaw, or built into a file that
  the Xteink X4 and X4 Pro load at their start, without a
  new firmware (see [Plugins as files](#plugins-as-files)).

Where a plugin's files go, what they are called and how big its assets
should be: [Plugin layout and best practices](plugin-conventions.md).

This guide walks through a code plugin, then lists what plugins can
contribute. Working with an AI assistant: the skill in
[`.claude/skills/paperpaw-plugin-development`](../.claude/skills/paperpaw-plugin-development/SKILL.md)
tells it how to write a plugin for PaperPaw as it is (without changing
PaperPaw), try it in the emulator and on an X4, and measure it; copy the
folder into a plugin's own project (or one's own skills) to use it
there. The SDK's API documentation (`cargo doc -p paperpaw-sdk
--open`) has every detail.

## The sample plugin

[`samples/hello-plugin`](../samples/hello-plugin/src/lib.rs) is a small
plugin to start from. It adds *Hello* to the tools menu, which opens a
screen greeting you by a name set in the settings, with the number of
books opened since the start. Try it in the emulator:

```sh
cargo run -p paperpaw-emulator --features hello -- --profile boox-go-7 --set hello.name=Ada
```

*Hello* is among the tools: open *Tools* in the main menu, then
*Hello*.

### The plugin

A plugin is a type implementing [`Plugin`]: a manifest and a `register`
function. `register` runs once at start-up and adds the plugin's
contributions through a [`Registrar`]:

```rust
pub const PLUGIN: Id = Id::new_static("hello");
pub const OPEN: Id = Id::new_static("hello.open");
pub const SCREEN: Id = Id::new_static("hello.screen");

pub struct Hello;

impl Plugin for Hello {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Hello", Version::new(0, 1, 0))
            .description("A sample plugin to start from.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_action(ActionSpec::new(OPEN, "hello.title").handler(|_, _, out| {
            out.push(Command::OpenScreen(SCREEN, Params::new()));
        }))?;
        r.add_menu_entry(MenuEntry::new(menus::TOOLS, OPEN).order(90))?;
        r.add_icon(OPEN, Cow::Borrowed("star"))?;
        // … the screen, a setting, an event, translations
        Ok(())
    }
}
```

Every id starts with the plugin's id (`hello.open`); `core` is the
kernel's. Titles are translation keys, translated where they are shown.

Icons are named as in [Tabler Icons](https://tabler.io/icons), any of
them (`paperpaw_sdk::icons` knows every name). PaperPaw's icon font holds
the ones it uses itself; for another, bring its drawing as an icon pack
(see below), else none shows.

### Reading and changing: `Env` and commands

Plugins never change the kernel's state directly. They read it through
[`Env`]: the device, the settings, the translations, the fonts, the
storage, the time, the battery, the passwords and keys. They change it
with [`Command`]s: open a screen or slot, go back, change a setting,
emit an event, run an action, show a short message, make an HTTP
request, keep a password, and so on. The kernel runs the commands after
the plugin's code returns.

A handler queues its commands in the `Commands` it is handed (`out` in
the examples): `out.push(..)`, `out.extend(..)`, `out.emit(..)`. It
reads as the slice of what is queued so far. Its fields are private, so
that it can grow: what PaperPaw adds to it later reaches every handler
without a new signature.

This keeps plugins simple to test and the API message-based. It does
not confine them: a plugin runs as native code without a sandbox (see
[Plugins as files](#plugins-as-files)).

### Screens

A screen implements [`Screen`]. `draw` paints it; `on_action` reacts to
actions (from buttons, or gestures bound to them); `on_gesture` to taps,
long presses and swipes; `on_event` to events while it is open:

```rust
impl Screen for HelloScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        // What the screen draws with: canvas, environment and style.
        let mut ui = ui::Ui::new(canvas, env, &self.style);
        self.header.draw(&mut ui, env.i18n().tr("hello.title"), None, None);
        // … the greeting, centred below
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::CONFIRM || *action == actions::BACK {
            cx.command(Command::Back);
            return Handled::Yes;
        }
        Handled::No
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        // A tap on the header's back arrow, or anywhere else, closes it.
    }
}
```

Screens are made by a factory each time they open
(`r.add_screen(ScreenSpec::new(id, title, factory))`). Return
`Handled::No` for what a screen does not handle: the kernel then runs
the action's own handler (e.g. the menu button opens the menu).

A screen opened again while it is open is not made twice: the kernel
goes back to the open one, closing the screens above it, and gives it
the params in `on_reopen`, so *Back* leaves it in one press. By default
that is one for each set of params (the details of another book open
above those of the first); `.instances(Instances::SINGLE)` keeps one at
most whatever the params (a transfer screen, a page of settings
roots), and `Instances::MANY` opens each time.

An action can be run with params: `Command::RunAction(id, params)`, or
`Command::run(id)` without. Its handler gets them as its second argument
(`.handler(|env, params, out| ..)`), and a screen reads them with
`cx.params()` in `on_action`. From a button or a gesture there are none.

A screen that needs time to pass asks for it with `cx.wake_at(when)`,
and `on_timer` comes then. Long work goes in steps, each asking for the
next wake. `on_close_request` may keep a screen open, for example to
ask first. `on_pointer` takes a finger whole, for drawing or moving
rows.

When its part of the screen changes size (the device turned, a window
resized, the row of hints or a bar came or went), a screen is made anew
by its factory for the new size, after `on_close`, which keeps what it
would lose. A screen that has much to keep (a book open, a long list
scrolled, a connection) lays itself out anew in `on_resize` instead,
from `cx.env().content()`, and answers `Handled::Yes`: it stays as it
is. A screen that shows one thing of the list below it (a book's
details, a message) says `Pane::DETAIL` in `pane`; where the screen is
expanded (a tablet, a phone on its side; never the X4) the kernel shows
it beside the list: the list at the start, each laid out for its part
(`on_resize`), a tap or `core.pane.other` moving keys between the two,
and a detail the list opens taking the place of the one shown. How much
room a screen has is `cx.env().screen_class()` (`ui::ScreenClass`:
compact, medium or expanded, by its width at the reader's text size) and
`cx.env().landscape()`; `events::DISPLAY_CHANGED` tells a plugin the
display's new size once its screens were laid out, for what it keeps at
a size outside them. `Screen::revision` is never
implemented: the kernel calls items added later only on screens built
with them. A screen that shows another inside it (`host::ScreenHost`)
answers `ScreenHost::revision` there, and calls the newer items of the
one it holds through `call_since`.

Some rules every screen should follow:

- **Touch and buttons alike.** Some devices have only buttons (the
  Xteink X4), some mostly touch (the Boox). Everything must be easy with
  either: buttons move a focus and confirm, taps act at once. Show the
  focus only once buttons are used (`ui::focus_shown`,
  `ListView::focus_on_keys`). The building blocks that take the buttons'
  actions and gestures (`on_action`, `on_gesture`) answer with a
  `ui::Response`: not theirs (`Ignored`), taken (`Same`), changed and to
  be drawn again (`Changed`), or done with a result (`Done`).
- **`draw` only draws.** On small devices a frame is drawn in bands
  (the X4 draws five), so `draw` runs several times for one frame, and
  again for every frame after it: whatever it reads or works out, it
  does over and over. Load what a screen shows when it opens or when
  that changes (in its factory, `on_show`, `on_event`); parse it, sort
  it, wrap long texts and make pictures small (`ui::Thumbnail`) then,
  keep the result in the screen, and only draw it in `draw`, which must
  not change state. It can skip what lies outside `canvas.clip()`. The
  one thing read while drawing is a picture too large to keep, such as
  one filling the screen: `ui::draw_image` reads its rows band by band.
  The emulator's `--trace` shows what each band of a frame reads;
  `crates/paperpaw-app/tests/draw.rs` checks every screen of the
  standard app.
- **Ask for redraws.** After a change, `cx.request_redraw()` (or a part
  of the screen) and `cx.request_refresh(mode)`: `Fast` for a moving
  focus, `Partial` for new content, `Full` to clear ghosting. A focus
  that moves asks for the two items it left and came to, never the
  whole screen: on the X4 a step then takes a fraction of a frame. The
  lists do it themselves (`ListChange::request` after `ListView`,
  `Rows` or `Sections` moved it); a focus of a screen's own asks with
  `ListChange::focus(left, reached)` (for buttons with
  `ui::button_reach`, as a focus frame goes round them). Where the focus
  changes more, such as a line about the focused item, ask for that
  part too.
- **Overlays** are screens with a `region`. One that takes the reader's
  attention (a menu, a question) washes out what lies below it
  (`dims_below`); one looked through (a keyboard over the field it
  types into, a notice) does not. A screen whose content is best in
  grays (a picture) says so (`grays`): panels with few grays then show
  it in more, as slowly as a full refresh. A screen drawn whole with
  pictures on it (covers, `ui::draw_image`, `ui::Thumbnail`) is shown so
  anyway where *Pictures in grays* is on; one that decides itself, or
  whose pictures change often, says no (`pictures_in_grays`).
- **Use the SDK's UI parts** (`paperpaw_sdk::ui`): headers, rows, lists,
  switches, numbers with − and +, messages, icons. They follow the user's
  appearance settings and line up with the rest of PaperPaw. What a
  screen draws itself takes its look from `UiStyle`: the focus with
  `ui.focus(..)` (a `ui::Ui`), what is on a focused row in `style.focus_ink(focused)`
  (white on the dark bar, black on a frame or a light bar), small shapes
  with `style.radius(height)` (pills where corners are round), boxes and
  cards with `style.corner()`, lines between rows with `ui.separator(..)`.
  The style's sizes are read with its methods (`style.margin()`,
  `style.gap()`, `style.row()` in pixels, `style.text()` and
  `style.small()` for text).

Screens say what they show, for reading the interface aloud,
automation and tests: the `ui` helpers (list rows, buttons, switches,
headers, labels) tell what they draw by themselves. A screen that draws
in its own way tells it with `canvas.describe(role, text, area)`, with
the role of what it is (`Row`, `Button`, `Switch`, `Check`, `Radio`,
`Field`, `Image`, `Link`, `Dialog`, `Progress` …), and says more where
it is so: `.focused(true)`, `.on(Some(true))`, `.disabled(true)` for
what does nothing now. Nothing is kept where nobody asks. A
screen reader of one's own hears where the focus of the buttons goes
(`events::FOCUS_CHANGED`: its role, label and area).

### Which building block for what

A plugin's screens are drawn with the SDK's `ui` module, so they look
like PaperPaw's own, follow the reader's text size and the look chosen
(a theme paints switches, the focus, buttons, headers, frames and more),
and work by touch and with the buttons alone. Sizes come from
`UiStyle::new(env)`: `margin`, `gap`, `row`, `header`, the text sizes
(`style.size(TextRole::BODY)` and the others). No block shortens a
title or a button's word: a title is measured first and keeps its whole
width, a box or control beside it takes what is left, and where that is
too little the row takes a second line (ask the block for its row's
height); only a value, the reader's own text in a field, may be
shortened. `paperpaw-dev ui-kit` draws every block below on every
device, in every look, light and dark, and checks that no label is
shortened.

| For | Use |
|---|---|
| The bar at the top, a title and a note (a page count) | `ui.header(..)`, `ui::Header` (with its arrows) |
| Parts one below the other, from the top and the bottom | `ui::Stack` (`take`, `take_bottom`, `rest`) |
| Equal columns side by side, e.g. two buttons | `ui::columns(area, count, gap)` |
| A list that pages and holds the focus | `ui::ListView`, `ui::list_step` |
| What a screen draws with | `ui::Ui` (`Ui::new(canvas, env, &style)`; the parts below are its methods: `ui.button(..)`, `ui.message(..)` …) |
| Text, on a line or wrapped | `ui::Label` (`Label::new(text).role(TextRole::SMALL).align(..).wrap()`) |
| Rows of a list | `ui::Row` (`Row::value`, `Row::switch`, `Row::number`, with `.note(..)`; `height`, `draw`, `hit`) |
| Rows of settings | `ui::draw_setting_row`, `ui::draw_stepper`, `ui::slider::Slider` |
| Sections with headings | `ui::sections::Sections`, `ui.heading(..)` |
| A grid of cells (covers, tiles) | `ui::CellGrid` (`cell`, `hit`, `step`) |
| A group on a card or in a frame | `ui.card(area, Frame::Card)` returns where its parts go |
| A button, on, focused, faint, with an icon | `ui.button(..)`, `ui.faint_button(..)`, `ui.icon_button(..)` |
| A few options to pick at a glance, or tabs | `ui::Segments` |
| Filters or tags in a row, each as wide as its word | `ui::Chips` (the height of their rows: `Chips::height_for`) |
| A one-line text in a row, typed on the keyboard | `ui.text_field(..)` (its row: `ui::text_field_height`), `ui::edit_text` |
| A list where several may be ticked | `ui::draw_check_row` (its row: `check_row_height`; the box alone: `ui::draw_check`) |
| Text longer than its room, scrolled by buttons and touch | `ui::TextView` |
| A date or a time | `ui::DatePicker`, `ui::TimePicker` |
| A box over the screen with a title, text and buttons | `ui::Dialog` |
| Asking for text, a folder, one of several options or yes or no | `ask::text`, `ask::folder`, `ask::choice`, `ask::confirm` (screens of their own) |
| A message where content would be, an empty list | `ui.message(..)`, `ui.empty(..)` (their heights: `ui::message_height`, `ui::empty_height`) |
| Progress | `ui.progress(..)` |
| An address to open on a phone, with its QR code | `ui::draw_address`, `ui::draw_qr` |
| Offering to go online where something needs the network | `ui::offline::OfflineOffer` |
| What the buttons do, at the bottom | the screen's `hint` (the kernel draws the row) |
| First-time hints over a screen | `help::add` (see [Hints for a screen](#hints-for-a-screen)) |
| A widget's frame | `widgets::draw_frame` |

Two buttons side by side, a margin from the screen's sides and bottom:

```rust
use paperpaw_sdk::ui::{self, Stack, UiStyle};

let style = UiStyle::new(env);
let margin = style.margin();
let screen = Rect::new(margin, 0, display.width - 2 * margin, display.height - margin);
let row = Stack::new(screen, margin).take_bottom(ui::Segments::height(&style));
let [ok, cancel] = ui::columns(row, 2, margin)[..] else { return };
let mut ui = ui::Ui::new(canvas, env, &style);
let shown = ui::focus_shown(env);
ui.button(ok, "OK", false, shown && focus == 0);
ui.button(cancel, "Cancel", false, shown && focus == 1);
```

Tabs over a page, the chosen one on, moved with left and right:

```rust
// Made with the layout; it keeps its words.
let area = Rect::new(margin, top, width, ui::Segments::height(&style));
let tabs = ui::Segments::new(&style, area, ["hello.books", "hello.shelves", "hello.notes"]);
tabs.draw(&mut ui, self.tab, self.focus_on_tabs.then_some(self.tab));
// on_action: if let Some(tab) = tabs.step(self.tab, action) { self.tab = tab; }
// on_gesture: if let Some(tab) = tabs.hit(gesture.start) { self.tab = tab; }
```

A dialog in the middle of the screen:

```rust
let mut dialog = ui::Dialog::new(env, &style, "Delete the book?", &["It goes for good."], &["Delete", "Keep"]);
dialog.place((display.height - dialog.height()) / 2);
dialog.draw(&mut ui, Some(self.focus));
```

### Settings

A setting is a constant with its id and the type of its value
(`Setting<bool>`, `Setting<i64>` or `Setting<Text>` for choices,
languages, lists and text). Its spec is declared in a `static` list,
which the plugin registers once. On the X4 the list stays in flash and
takes next to no memory:

```rust
pub const NAME: Setting<Text> = Setting::new("hello.name");
pub const DATE: Setting<bool> = Setting::new("hello.clock.date");
const GROUP: Id = Id::new_static("hello.clock");

static SETTINGS: [SettingSpec; 3] = [
    SettingSpec::text(NAME.static_str(), "hello.setting.name", "", GROUP)
        .description("hello.setting.name.help"),
    SettingSpec::switch(DATE.static_str(), "hello.setting.date", true, GROUP),
    SettingSpec::choice("hello.clock.face", "hello.setting.face",
        &["hello.face.plain", "hello.face.ticks"], "hello.face.ticks", GROUP)
        .description("hello.setting.face.help"),
];

r.add_settings(&SETTINGS)?;

let name: &str = env.settings().read(&NAME);
out.push(Command::set(&NAME, "Ada".into()));
```

`SettingSpec::switch`, `number`, `choice`, `language`, `list` and
`text` make them, each with its id, title, default and group;
`SettingSpec::new` takes any kind and an `Id`, for a spec whose id is
known only at run time (registered with `add_setting`). Every builder is
`const`. The settings screens list every setting by its group, with its
description below it; the kernel saves changes to `paperpaw.ini`.
`advanced()` hides rarely needed ones, and `scoped()` lets one be set per
book. Settings known only at run time (one per widget, say) are read by
their ids, `env.settings().text("…")` and so on. Passwords and keys do
not belong in settings: see [Passwords and keys](#passwords-and-keys).

What few settings need goes into a `static SettingExtra`, which a
setting takes with `with_extra(&EXTRA)`: a unit, a step, a section, a
slider, an editor screen of your own, or being a secret.

```rust
static WHILE_SYNCING: SettingExtra = SettingExtra::new().shown_if(Id::new_static("hello.sync"), "on");
static SERVER: [SettingSpec; 1] = [
    SettingSpec::text("hello.server", "hello.setting.server", "", GROUP).with_extra(&WHILE_SYNCING),
];
```

`shown_if` shows a setting only while another has a value. Here the
server's address shows while syncing is on. `check(fn)` checks a value
before it is set. A value it refuses is not set, and the reader sees
the translation of the key it returns. Screens of your
own that list settings open them as the settings pages do with
`ui::setting_opens` (its editor, the keyboard for text, the list of a
choice's values); text typed there comes back to the kernel as
`events::SETTING_TYPED`, which sets it. They lay their rows out as the
settings pages do, so that no title is cut for its control:
`ui::row_lines` tells once, when the list is made, whether a row's
control goes beside its title or below it and how high the row is
(`ui::setting_height` the height alone), and `ui::draw_number_lines`,
`ui::draw_stepper_lines`, `ui::draw_switch_lines` and
`ui::draw_setting_row` draw it there (`RowLines::at` moves it to the
row's place).

### Events

```rust
r.on(events::BOOK_OPENED, move |env, reading, out| {
    opened.set(opened.get() + 1);
    // reading.path, reading.title, reading.progress, reading.at() …
})?;
```

The kernel and plugins emit events; any plugin can listen. An `Event<P>`
names an event and what it carries, a struct made with `event_data!`,
so emitting (`EVENT.emit(data)`) and hearing (`r.on`) are checked as
the plugin is built; screens get every event too and read one with
`EVENT.read(event, params)` in `Screen::on_event`. Standard ones are in
`builtin::events`: started, a setting changed, a book opened, a page
turned, a book moved, books came into the library or left it, its
bookmarks or highlights changed, a word looked up, a place in the open
book to show, text chosen, notes exported, the device suspends or
resumes, PaperPaw stops (a restart: save as at suspending, but it is no
sleep). Plugins define their own the same way (in their namespace),
and `EVENT.schedule(data, at)` emits one at a time (work at a time,
below). Underneath, what events carry travels as named
strings (`Params`): events known only at run time are heard by id with
`r.subscribe`.

Answers come as events too, typed in `replies`: a plugin names the
event it wants, e.g. `const FETCHED: Event<HttpReply> =
Event::new("news.fetched")`, and passes it with the request.

To ask the reader, `ask::text("Server", &ENTERED).initial(url).command()`
opens the keyboard and brings back a `Typed`. `ask::folder` brings back
a `FolderChosen`, `ask::choice` a `Picked` and `ask::confirm` a `Confirmed`.
A value passed with `.with(key, value)` comes back in the answer's
`context`, so a plugin tells its questions apart. A `Picked` has no
`context`: on `ask::choice`, `.about(text)` comes back as its `path`.

What is known of books is a service, not a screen's: `library` has a
book's details (title, author, series, tags, cover, progress, and
fields plugins add with `library::set_field`), the books read last and
the favourites, so any plugin can show or use them. Views of many books
read the catalog rather than each book (below, "What every library
offers"), show covers made small and kept on the storage
(`library::cover_variant`, a few kilobytes each, not the picture, one
for each size a view shows) and take a book's details with the size a
listing told (`library::listing`, `library::book_sized`): on a card,
each file looked up again is a search through its folder.

The bookmarks and highlights of books are shared: `annotations` reads
and keeps them, and whoever changes them emits
`annotations::changed(book)`, so the reader shows them anew and syncs
pass them on (the `koreader` plugin, for example).

State a plugin keeps between events lives in an `Rc` it hands to its
handlers when it registers.

### Translations

```ini
[meta]
language = en
name = English
plural = one-other

[messages]
hello.title = Hello
# {name} is the name set, {count} the books opened since the start.
hello.greeting.one = Hello, {name}! You opened {count} book.
hello.greeting.other = Hello, {name}! You opened {count} books.
```

Every text a plugin shows comes from catalogs like this, added with
`r.add_translations(…)`; English first, then any other language. `tr`
translates, `format` fills `{placeholders}`, `plural` picks the form for
a number. A test that all languages have the same keys belongs in every
plugin (see the sample).

Two messages say what a plugin is in the lists: `<id>.plugin.name` and
`<id>.plugin.description` (a line). *How it works* says more: a few
sentences on what it does, where it is found and how it is used, plain
text with paragraphs apart by an empty line, one file per language
beside the catalogs (`how/en.txt`, `how/de.txt`). The manifest names
them; as a table in the program they take no memory until the plugin
manager shows one (a plugin held), paged like a book:

```rust
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];
// In `manifest`: Manifest::new(…).description(…).how(&HOW)
```

The store shows the same text on the plugin's page and in the readers'
store: put the files into the folder you upload as well (`how/en.txt`,
`how/de.txt`), as PaperPaw's own plugins in the store have them, or say
it in the entry's `description` (see [Publishing](#publishing)). A data plugin
gives it as `<id>.plugin.how` in its catalogs, quoted, paragraphs apart
by `\n\n`. `paperpaw-dev` warns where a plugin with code has none or a
short one.

### Testing

Plugins are tested with the real kernel and an in-memory storage, as the
sample's [test](../samples/hello-plugin/tests/hello.rs) does. The kernel
comes from the crate `paperpaw-core`, a dev-dependency. Build a kernel
with the plugin and send it input. Then look at the open screen, the
settings, the files and the HTTP requests it makes (`Kernel::take_io`).
Answer them with `Kernel::http_response`. The emulator then shows how it
looks and how much memory it needs on each device:

```sh
cargo run -p paperpaw-emulator --features hello -- --profile xteink-x4 --headless --script "run:hello.open" --screenshot out.png --heap-report
```

Before a plugin is published, `paperpaw-dev` checks all of it on every
device: see [Checking your plugin](#checking-your-plugin).

## Checking your plugin

`paperpaw-dev` ([`tools/dev`](../tools/dev/src/lib.rs)) looks at a
plugin the way its users will meet it, before it is published. It runs
PaperPaw's standard app with the plugin added on emulated devices (by default
the X4, X4 Pro and Boox profiles), draws everything the plugin adds and
checks it. It finds what
is easy to miss on one's own device: a label that fits in English but not
in German, a widget that spills out of its cell at 1x1, a screen that
cannot be left with the X4's buttons, a frame that reads the card in
every band.

What it is given:

- **A plugin with code**, in its own repository: a few lines add it to
  the app (see [below](#in-a-plugins-own-repository)), then
  `cargo run --example paperpaw-dev -- check --plugin my-plugin`.
- **A data plugin**: its folder, `paperpaw-dev check path/to/my-theme`.
- **A code plugin built for a device**: `paperpaw-dev package hello.ppn`.
- **A code plugin's library** (interface 7, `.so` for the desktop;
  Android loads no code plugins): `paperpaw-dev package libmy_plugin.so` runs the loader's check
  before the plugin does: a 64-bit little-endian shared object for x86-64 or
  AArch64 that names no other library (`readelf -d`: no `NEEDED`), imports
  nothing (`nm -D --undefined-only`: empty) and exports exactly
  `PAPERPAW_ENTRY` and `PAPERPAW_METADATA` (`nm -D --defined-only`).
- **A code plugin's source**: `paperpaw-dev package path/to/crate` reads its
  `src/` for the conventions of
  [Plugin layout and best practices](plugin-conventions.md): files over 4 KB
  embedded in the code, writes under `plugins/<id>/`, ids outside `<id>.`.
  Only warnings.
- **PaperPaw's own plugins**, which the app has already:
  `cargo run -p paperpaw-dev -- check --plugin widgets`.

`check` runs every command below and writes one report into
`target/paperpaw-dev` (`--out` for another folder): `report.html` with
every finding, its picture and the place marked, `report.json` for tools,
and the pages the commands made. Errors make the run fail, for CI;
warnings too with `--strict`.

```sh
cargo run -p paperpaw-dev -- check --plugin widgets --quick
cargo run -p paperpaw-dev -- visual --plugin stats --profile boox-go-7 --language de
cargo run -p paperpaw-dev -- gallery samples/plugins/airy-theme
cargo run -p paperpaw-dev -- --help
```

### Variants

A plugin is looked at in every combination of device profile
(`--profile <id>|all`; `paperpaw-dev --list` lists them), language
(`--language <code>`, by default every language the app has; `pseudo`
for the pseudo-language), light and dark (`--light`, `--dark`) and look
(`--theme <name>|all`: the default, square corners, round corners with
strong contrast). `--quick` takes one variant a device: English, light,
the default look. `--set <id>=<value>` sets a setting for every run.

Plugins' requests go to the network, unless `--network fixtures`: then
the emulator answers them offline, alike everywhere, from its own
answers for PaperPaw's plugins (a plugin library, an OPDS catalog, the
weather and others; `shells/desktop/src/offline.rs`), and every other
request fails. The screens that show what those answers give are drawn
with it, and `reach` takes a screen that stays out of reach while its
requests went unanswered for an error: it waits for data, it is no dead
end. PaperPaw's CI checks its screens so.

### The commands

| Command | What it does |
|---|---|
| `gallery` | Contact sheets: every screen the plugin adds and every widget in every size it can take, in every variant, as HTML pages (`gallery/index.html`) and one PNG sheet a plugin and device. |
| `visual` | What is drawn, checked: text cut at its box or the screen (`visual.text-cut`), shortened (`visual.ellipsis`), an explanation of several lines cut short (`visual.explanation-cut`, an error) or broken where it should not be (`visual.wrap`); ink outside a widget's cell (`visual.outside-cell`) or the screen (`visual.off-screen`); parts over each other (`visual.overlap`); margins off the interface's (`visual.margin`) and rows out of line (`visual.align`); labels and icons off centre by their ink (`visual.ink-centre`); touch targets under 7 mm (`visual.touch-target`); buttons without the room around them (`visual.button-room`); a focus that does not show with buttons (`visual.focus`); text that dithers away on black and white or is too light on grays (`visual.contrast`). |
| `lint` | Texts and ids: translations missing or left over, placeholders and plural forms; keys shown as they are because English has no text for them (`lint.missing-key`); settings without help (`lint.setting-help`), and descriptions longer than the two lines of their row on the X4 (`lint.help-cut`); setting groups without an order or an icon (`settings.group-order`, `settings.group-icon`), and a widget's settings in a group of the main page rather than the widget's own (`settings.widget-group`); plugins with code that say nothing or little of how they work (`lint.how-it-works`); ids outside the plugin's namespace; dashes (`lint.dash`); and in the pseudo-language, text that fits in English but not once it is longer (`lint.pseudo-overflow`) and text drawn without going through the catalogs (`lint.hardcoded`). |
| `reach` | With buttons alone: every screen the plugin adds can be reached from the home screen (`reach.unreachable`) and closed with *Back* (`reach.no-close`), the focus visits every row and button (`reach.focus-skips`) and does not get stuck (`reach.focus-trap`); a screen whose focus moves but which tells nothing of its rows (`reach.undescribed`). |
| `explore` | A cheap baseline beside the flows: every target of every screen the plugin adds (at most 20 a screen, 200 a device) used once, each from a fresh start, by touch or with the buttons; then the home screen must be reached again. A panic, an allocation the device would refuse or no way back home is an error (`explore.failed`). |
| `perf` | Budgets on the X4: storage read while drawing (`perf.draw-io`, the rule of `crates/paperpaw-app/tests/draw.rs`), the heap a screen takes against what the X4 has free (`perf.heap`), heap kept after it closes (`perf.leak`), frames estimated slow on an X4 (`perf.frame-time`), many reads before the first frame (`perf.open-io`); the top costs in `perf/index.html`. |
| `flows` | Scripted flows with assertions (`--flow a.flow`): each step's heap and frame time against the X4's budgets, and each flow played again at the edges; see below. |
| `package` | A data plugin as the device's installer and the plugin library take it (id, paths, file count and sizes, file kinds, `formats`, settings and presets that exist), installed on each device as the store does; a `.ppn` file checked as the device checks it (header, digest, the author's signature), with its fingerprint and sizes; flows and test pictures left in a data plugin's folder, which would ship with it (`package.tests`). `--release <version>` (for a `.ppn`, the release's fingerprint) tells whether that release takes it. |
| `snapshots` | Screens and widgets compared with reference pictures (`--snapshots <dir>`, `tests/snapshots` by default); `--bless` writes them anew. |
| `ui-kit` | Every building block of the SDK's `ui` module (see [Which building block for what](#which-building-block-for-what)), drawn as the gallery draws screens: on every device, in every look, light and dark, with and without the focus. Not part of `check`. |

The pseudo-language makes every English text about a third longer and
accents its letters (`[Šéţţîñĝš ~~]`); with `--language pseudo` the
gallery and the visual checks show where a translation will not fit.

### Flows

A flow is a text file of steps and of what should be true after them, run
on every device it fits:

```text
# Night mode from the menu, with buttons alone.
profiles: xteink-x4, xteink-x4-pro

long:confirm
expect screen menu.list
expect focus "Home"
right
right
right
right
expect off "Night mode"
confirm
expect setting core.night-mode = on
back
expect stack 1
expect no warnings
```

Steps are those of the emulator's scripts: a button's name, `long:`,
`tap:x:y`, `tap-on:<text>`, `long-tap:x:y`, `swipe:`, `down:`, `move:`,
`up:`, `wait:<ms>`, `run:<action>`, `open:<screen>`, `type:<text>`
into the keyboard shown, `type-enter:` to confirm it, `redraw` and
`shot:<file>`. Lines before the first step may say `profiles: a, b`
and `set: <id> = <value>`, a setting before the app starts. Assertions
are `expect screen`, `stack`, `text`, `no text`, `focus`, `on`, `off`,
`setting`, `no warnings` and `picture <name>`, which compares the
screen with a snapshot. `paperpaw-emu --record
my.flow` writes what one does in the emulator's window as a flow, to be
replayed with `paperpaw-dev flows --flow my.flow`; the `expect` lines are
added after.

Write a flow for every action of a plugin: each thing a user does with it
(a setting switched, an entry made, a sync started, a book opened from its
screen), from where they start to what should be true after, by touch and
with the buttons alone. Flows are the main test of what a plugin does;
`explore` only finds what breaks when each thing is touched once.

On the profiles with the X4's budgets every step is measured: the most
the heap held during it and its slowest frame, estimated on an X4. The
costliest step is named: `flows.heap` is an error where the X4 would run
out and a warning above an eighth of its heap, `flows.frame-time` a
warning above half a second.

Unless `--quick`, each flow is then played again in variations that find
what happens at the edges: every step done ten times; *Back*, *Home* or
sleep and waking at a few places in the middle; network answers that
fail, come late, are empty or far too large; the pseudo-language; a book
with a very long name; a storage that is almost full. Assertions are left
out there. Each variation must not panic, must stay within the heap and
must find its way back to the home screen (`flows.varied`).

Keep flows and their snapshots beside the plugin rather than in its
folder (in a plugin's own repository, `tests/flows` and
`tests/snapshots`): `package` warns of those that would ship in a
package. None of this is part of a device's build; `paperpaw-dev` runs on
the computer only.

### In a plugin's own repository

[`tools/dev/templates`](../tools/dev/templates/README.md) has what a
plugin's repository needs: the harness, the snapshot test and a CI job.
`paperpaw-dev` is a dev-dependency by git, with the tag of the release the
plugin is made for, and the SDK is patched to the same tag, so that the
plugin and the app share one:

```toml
[dev-dependencies]
paperpaw-dev = { git = "https://github.com/hthoene/paperpaw", tag = "v0.6.0-beta.3" }

[patch.crates-io]
paperpaw-sdk = { git = "https://github.com/hthoene/paperpaw", tag = "v0.6.0-beta.3" }
```

`examples/paperpaw-dev.rs` adds the plugin to the app:

```rust
fn main() -> std::process::ExitCode {
    paperpaw_dev::main(|app| app.plugin(my_plugin::MyPlugin))
}
```

`tests/snapshots.rs` compares the cases of `tests/snapshots/cases.ini`
with the pictures beside it; `PAPERPAW_BLESS=1 cargo test --test
snapshots` writes them anew (look at each before committing it):

```ini
[x4-screen]
screen = my-plugin.screen
profile = xteink-x4

[boox-widget-wide]
widget = my-plugin.widget 4x2
profile = boox-go-7
core.night-mode = true
```

The CI job (`.github/workflows/paperpaw-dev.yml`) runs the tests,
`check --plugin my-plugin --quick` and the flows of `tests/flows`
(`flows --flow tests/flows`, varied at the edges), and keeps the report
as the run's artifact, also when a check failed.

## What plugins contribute

| Contribution | Registrar | What it is |
|---|---|---|
| Actions | `add_action` | Something to do, bound to buttons and gestures, listed in menus |
| Screens | `add_screen` | Full screens and overlays (menus, dialogs, panels) |
| Slots | `add_slot`, `bind_slot` | Roles a screen fills: home, reader, library, menu, settings, keyboard, dictionary, … |
| Menu entries and sections | `add_menu_entry`, `add_menu_section`, `place_menu_entry`, `hide_menu_entry` | Actions, settings and submenus in any menu, by order or beside other entries (`MenuPlace`: after, before, in a section, first, last); another plugin's entry taken out. A menu the user redefined shows entries added later too |
| What entries show now | `set_item_state`, `describe_item` | A label, a switch, faint, hidden or a badge of the plugin's own entries as things are now (`ItemState`); a line under an entry |
| Icons | `add_icon` | The icon of an action, setting group or menu; of a semantic icon (`icons::BACK`, `NEXT`, `SHIFT`, …) for a theme; `U+F8000` names a code point of one's own |
| Settings and groups | `add_setting`, `add_setting_group` | Shown and saved by the kernel. A group names its order in the sections of the main page (`SettingGroupSpec::order`: books 1 to 49, the device 51 to 199, customising from 201) and an icon (`add_icon` with its id); one that names no order is listed at the end of customising, by name, with a plugin's icon |
| Setting defaults | `set_setting_default` | New defaults for other plugins' settings (themes) |
| Event handlers | `on`, `subscribe` | React to events |
| Key maps | `add_keymap`, `add_keymap_preset`, `add_keymap_when` | What buttons, taps and swipes do; bindings that apply while a switch is on |
| Formats | `add_format` | Open a kind of file as a document to read; big pictures may be decoded when their page shows (`Document::prepare_image`); files without a known extension told by their first bytes (`FormatSpec::sniff`); the format's own settings in `OpenOptions::setting`; a document still coming says `DocumentError::NotReady`, then `events::DOCUMENT_READY` |
| Lookup providers | `add_lookup_provider` | Where words are looked up (a glossary, a translator, Wikipedia): the dictionary's panel lists every provider's entries, those that go online when asked; an entry may open a screen with more of it (`LookupEntry::more`) |
| Reading apps | `add_reader` | A screen of the plugin that reads books of some formats (a comics reader), offered where books open and chosen per format, folder or book |
| Voices | `add_voice` | Speak text aloud in place of the system's voice, for the languages it speaks, chosen with `settings::VOICE` |
| Fonts | `add_font`, `add_font_file`, `bind_font` | Fonts and the roles they fill |
| Hyphenation | `add_hyphenation` | Patterns for a language |
| Text filters | `add_text_filter` | Change a book's text before it is laid out |
| Decorators | `add_decorator` | Mark words on book pages (a background, a line, a frame), drawn the same by every reader |
| Page layers and frames | `add_page_layer`, `add_page_frame` | Draw on book pages beside the words, with room of one's own; set pages in columns |
| Input filters | `add_input_filter` | See every button and finger first: pass it, take it, or run an action instead |
| Refresh filters | `add_refresh_filter` | Change how the panel is refreshed, e.g. in full on a screen of your own |
| Status items, lines and places | `add_status_item`, `add_status_line`, `add_status_place` | What status lines show, what a tap on an item does, how much it matters where a line is full; places where items show (`status::Place`) |
| A look | `set_ui_look` | Another look for switches, the focus, separators, buttons, headers, button hints, progress bars, sliders and widgets' frames on every screen (`ui::look`) |
| Widgets and their places | `add_widget`, `add_widget_place`, `add_widget_category` | Blocks for the home screen and the sleep screen; places that show widgets (`widgets::WidgetPlace`), in a grid or above each other (`widgets::WidgetStack`), which plugins ask whether they show theirs (`Widgets::shown`); kinds of widgets of one's own, with their icon |
| Setup steps | `add_setup_step` | A page of the first-run setup: a setting to choose or a screen of the plugin's, where the device needs it |
| Services | `provide` | Anything one plugin offers others, found with `env.services().get` |
| Contributions | `contribute` | Additions to a point another plugin defined |
| Translations | `add_translations` | Catalogs for any language |
| Book data | `add_book_data`, `add_book_cache` | Where the plugin keeps something for each book (named by `library::key`, in the folder itself or in its subfolder of the key's first two digits, `<folder>/<2 hex>/<prefix><key>…`, so that no folder holds thousands of entries): moved and removed with the book. What `add_book_cache` names (made again from the book when needed) is cleaned up by itself when the book is gone; what `add_book_data` names (the reader's own: notes, places) only when the reader agrees, in *Clean up storage* |
| Book fields | `add_book_field` | A value kept with books (`library::set_field`), shown on their information page and, where `in_catalog`, kept in the catalog to filter by |
| Book badges | `add_book_badge` | A few letters or an icon on books' rows in views of the library |
| Actions of others | `override_action` | Runs in place of any action's own handler; of several, the highest priority wins |
| Inputs and gestures of one's own | `add_input`, `add_gesture` | `input.<id>` and `gesture.<id>`, bound in key maps like any other |
| Bars across screens | `add_chrome` | Tabs, a dock or a bar along an edge that stays while screens open and close (`chrome`) |
| Watching | `observe_actions`, `observe_events` | Sees every action before it runs, and every event after its handlers |
| Menus by device | `set_menu_need` | Offers an entry only where the device can do it, e.g. speak |
| Points and events declared | `add_point`, `declare_event` | Says what a point takes and what an event carries, for authors and for rules |

Slots are how a plugin replaces another: bind your screen to
`core.home` and it is the home screen; the user's
`[slots]` configuration has the final word. Where several plugins offer
a screen for a slot, *Settings, Plugins, Screens* lets the reader choose
one (or *Automatic*, the plugins' order), and a plugin newly installed
that offers one asks once whether to use it now. `Env::slot_choice`,
`slot_title` and `screen_title` tell a screen of one's own the same.
Built-in slots are in
`builtin::slots`, built-in menus in `builtin::menus`: the main menu
(`core.main`), its tools (`core.tools`) and the settings' pages
(`core.settings-pages`).

```rust
// A home screen of one's own: preferred over the shipped one.
r.add_screen(ScreenSpec::new(SHELF, "shelf.title", Box::new(|env, _| Box::new(Shelf::new(env)))))?;
r.bind_slot(slots::HOME, SHELF, Priority::PREFERRED)?;
```

### What a slot's screen does

A screen bound to a standard slot takes the parameters the others open
it with and answers as they expect, so that every plugin works with it:

| Slot | Opened with | The screen |
|---|---|---|
| `core.home` | nothing; at the start and by `actions::HOME` | is the bottom of the stack: opened again, the screens above it close. It leads to the menu and the library by touch and by buttons alone. |
| `core.library` | nothing; by `actions::LIBRARY` | opens a book with `readers::open` (the reading app chosen for it). For a book it offers the menu `menus::BOOK`, as `Menus::shown_items` has it: when an entry is chosen it emits `events::BOOK_CHOSEN` with the book, then runs the entry's action. |
| `core.settings` | nothing: the groups; `params::ID`: the settings of that group; `params::FOCUS`: the page of that setting, focused on it | changes settings with `Command::SetSetting`, and a setting with an editor (`SettingExtra::editor`, a screen or a slot) by opening that (`Editor::open`). |
| `core.menu` | `params::MENU`, the menu; `params::HIDE`, entries to leave out this time | shows `env.menus().items(menu)`; for an entry chosen it closes (`Command::Back`) and runs its action (`Command::RunAction`); a setting listed there it changes, or opens its editor. |
| `core.choose-folder`, `core.choose-value` | `params::ID`, the setting | writes the folder, or the value, chosen to the setting. Name the slot as the editor (`SettingExtra::editor_slot`) to have it edited there by whichever plugin fills it. |
| `core.choice` | `params::TITLE`, `PATH` (what it is about), the options, `REPLY`; asked with `ask::choice` | shows the options; for one chosen it closes and emits the reply as a `Picked` with the option and `path`; *Back* or a tap outside closes without one. |
| `core.confirm` | `params::TITLE` (the question), `TEXT`, `ui::confirm::YES_LABEL`, `NO_LABEL`, `FOCUS_YES`, `REPLY`; asked with `ask::confirm` | asks yes or no, the focus on no unless `FOCUS_YES`; for an answer it closes and emits the reply as a `Confirmed` with all of its parameters; *Back* or a tap outside closes without one. |
| `core.keyboard` | `params::TITLE`, `TEXT` (to start from), `MODE` (`text`, `number`, `password`), `REPLY`, `LIVE` | on confirming, closes and emits the event named by `reply` as a `Typed` with all of its parameters; with `live`, on each change too, with `Typed::typing` set; *Back* closes without an answer. `ask::text` asks this way. |

The documentation of `builtin::slots` says the same for every slot.

### Which one wins

- **Slots.** The screen the user chose in `[slots]` (*Screens*,
  above); else the binding
  with the highest `Priority` (`FALLBACK`, `DEFAULT`, `PREFERRED`, or
  any number); between equal priorities, the one registered later,
  that is of the plugin loaded later.
- **Key bindings.** A later source overrides an earlier one for the
  same button or gesture: the plugins' `add_keymap`, in the order the
  plugins load; then their `add_keymap_when` while its switch is on, so
  these override the plugins' plain bindings; then the device's, the
  preset the user chose, and the user's own `[keymap.*]`. A plugin's
  binding never overrides the device's, the preset's or the user's.
- **Menus.** Entries in their `order`, equal orders in the order they
  were added; a menu the user redefined in their order, with entries
  contributed since after their neighbour.

### Pressing twice

A button, or a tap zone, can do one thing when pressed once and another
when pressed twice: bind the second as `double.<button>` (or
`double-tap.<zone>`) in a key map, in `paperpaw.ini` or on the device
(*Buttons and gestures*, "Buttons pressed twice"):

```ini
[keymap.global]
double.confirm = core.home
```

How quickly the second press must follow the first is the setting
*Double tap* (`core.double-tap-ms`, 300 ms; advanced). A button waits for
a second press only if something is bound to its double press, so the
buttons that have none respond at once. Where one is bound, the single
press is answered after that time instead of at once, as the second press
might still come. Chords (two buttons at once) are not built.

### Examples

A format: files with these extensions open as a `Document`.

```rust
r.add_format(FormatSpec::new(
    Id::new_static("notes.format"),
    "notes.format",
    vec![Cow::Borrowed("note")],
    Box::new(|storage, path, _options| Notes::open(storage, path)),
))?;
```

A status item: what a status line shows, `None` to hide it. A tap on
it runs an action of its own plugin, here one that opens the notes.
Where a line is full, items of a low priority give way first.

```rust
r.add_status_item(
    StatusItemSpec::new(
        Id::new_static("notes.count"),
        "notes.count",
        Box::new(|env, _| Some(StatusValue::Text(count(env).to_string()))),
    )
    .tap(Id::new_static("notes.open"))
    .priority(96),
)?;
```

An input filter: it sees every button and finger first.

```rust
r.add_input_filter(InputFilterSpec::new(
    Id::new_static("lock.filter"),
    "lock.filter",
    Box::new(|env, _input| if locked(env) { InputVerdict::Take } else { InputVerdict::Pass }),
))?;
```

A request and its answer: the reply event brings the `HttpReply`.

```rust
const FETCHED: Event<HttpReply> = Event::new("news.fetched");
// In an action or a screen:
out.push(Command::http(HttpRequest::get("https://example.org/feed", &FETCHED)));
// When registering:
r.on(FETCHED, |_env, reply, out| {
    if reply.status == 200 {
        out.push(Command::Notify(headline(&reply.body)));
    }
})?;
```

A timer: the work's event comes at that time, here each hour.

```rust
const TICK: Event<()> = Event::new("weather.tick");
out.push(Command::work(
    Work::at(&TICK, env.now().add_millis(60 * 60 * 1000)).every(Duration::from_secs(3600)),
));
r.on(TICK, |_env, _, out| out.push(Command::http(HttpRequest::get(FORECAST, &FETCHED))))?;
```

### What every reader offers

A plugin that works with books never names the reader plugin: it uses
what `builtin` defines for every reader, so a reader of one's own (bound
to `core.reader`) replaces the shipped one in full. A reader of some
formats only registers as a reading app instead (`readers::ReaderSpec`,
`Registrar::add_reader`): books open in the app chosen for them, and
whatever opens a book opens it with `readers::open(path)` (never by
opening `slots::READER` itself). The reader chooses with
`readers::ask_once` (*Open with …* in the book menu: this time, or kept
when asked; plugins add other ways to open a book to `menus::OPEN_WITH`),
`readers::ask` (a book's or a folder's reading app, kept) and in the reading
settings (`settings::READING_APP`, and one per format that more than one
app reads). A reading app's screen is its own; one that cannot open a
book after all answers `readers::cannot_open` instead of an error of
its own, and the next app in that order opens it. One reading app is
open at a time. What moves folders calls `readers::folder_moved`, so
that their choices go along; a shell opens a book with
`Kernel::open_book`.

| What | Standard | |
|---|---|---|
| Show a place in the open book, a part marked | `events::GOTO` (`Goto`) | readers take it; tables of contents, bookmarks, search and syncs send it |
| Text chosen, and the action chosen for it | `menus::SELECTION`, `events::TEXT_CHOSEN` | add an action to the menu, hear the event |
| Notes exported as Markdown | `events::NOTES_EXPORTED` | for notes apps and syncs |
| The reader's menus | `menus::READER`, `READER_MORE`, `READER_PANEL` | add entries there |
| The type of pages | `settings::FONT_SIZE`, `FONT_WEIGHT`, `TEXT_CONTRAST`, `SMOOTH_TEXT`, `WORD_SPACING`, `LETTER_SPACING`, `KERNING`, `LIGATURES`, `LINE_SPACING`, `LINE_BREAKING`, `KOREAN_BREAKS`, `MARGIN`, `MARGIN_ENDS`, `JUSTIFY`, `HYPHENATE`, `HYPHEN_MIN_LEFT`, `HYPHEN_MIN_RIGHT`, `INDENT`, `INDENT_SIZE`, `PARAGRAPH_SPACING`, `PARAGRAPH_SPACE`, `WIDOWS`, `ORPHANS`, `PUBLISHER_STYLES`, `BOOK_ALIGN` and the other `BOOK_*`, `FOOTNOTES` | the kernel's settings (each book may have its own), which readers read |
| The page's words | `events::PAGE_WORDS_WANTED`, `reading::PageWords` | readers answer `reading::ask_page_words` |
| Where a book was left | `positions::{load, save}`, `Reading::xpointer` with `BOOK_CLOSED` | readers keep it; syncs and lists read it without opening the book |
| The contents, search, the marks, a footnote, skimming | `slots::TOC`, `SEARCH`, `ANNOTATIONS`, `FOOTNOTE`, `SKIM`, `events::FOLLOW_LINK`, `SKIM`, `SKIMMED` | bind a screen of one's own; readers open the slots, follow the links and go where skimming asks |
| A book opened as the reader opens it | `reading::open_options`, `reading::conversion` | its places fit the reader's (a search, contents, an XPointer) |
| Pages that go right to left | `Metadata::direction` (`PageProgression`), `Screen::direction` | readers mirror taps and swipes (an EPUB's `page-progression-direction`); other screens follow the interface (`Direction::Auto`) |
| Status lines' settings | `groups::STATUS_LINES` | the group readers put them in |
| Reading aloud's settings | `group_sections::ALOUD` | the section readers and shells put them in |
| Key bindings while reading | the layer `[keymap.core.reader]` | the slot's layer, whichever screen fills it |

A reader in turn sends the reading events (`BOOK_OPENED`, `PAGE_TURNED`,
`BOOK_CLOSED`, `BOOK_FINISHED`), takes `GOTO`, and offers its actions
in these menus.

### What the other slots promise

A screen bound to one of these slots replaces the shipped one; it is
opened with what the table says and keeps its promises, so the plugins
around it go on working.

| Slot | Opened with | Promises |
|---|---|---|
| `slots::HOME` | nothing | The start screen; where it lists things to open, it lists `menus::APPS` and `menus::TOOLS` |
| `slots::MENU` | `params::MENU`, `params::HIDE` (entries left out this time) | The menu's entries as `Menus::shown_items` has them, with their state and the line under them; runs an action, changes a setting in place, opens a submenu |
| `slots::PANEL` | `params::MENU` | The same as controls for quick changes (sliders, switches) |
| `slots::SETTINGS` | `params::ID` (a group), or nothing for all | The settings by group and `menus::SETTINGS`; a choice with fewer than two values is not listed |
| `slots::KEYBOARD` | `params::TITLE`, `TEXT`, `MODE` (`text`, `number`, `password`), `REPLY` | Emits the event `reply` with every parameter it was opened with and `text` set to what was typed; nothing when cancelled |
| `slots::LOOKUP` | `params::WORD`, `LANGUAGE` | The entries of every lookup provider that knows the language (`lookup`), those that go online only when asked; an entry's `more` opened on request, by touch and with the buttons |

Points of the shipped plugins, for `Registrar::contribute`:

| Point | A line each | |
|---|---|---|
| `keyboard.layouts` | `id; code; letter rows; variants`, e.g. `dutch.layout; NL; qwertyuiop asdfghjkl zxcvbnm; e=éèë` | a layout to choose, named by the translation of its id |
| `about.lines` | a paragraph (Markdown) | shown on *About PaperPaw*, below the address |

A reader of one's own builds on the shipped one's parts in
`paperpaw_sdk::reading` rather than copying them: `FilteredDocument` (a
book through the plugins' text filters; kept `DroppedRuns` read the
blocks they leave out once), `PageIndex` (the starts of its
pages, kept on the device), and with the feature `unstable` (as the
layout, until 1.0) `PageType` (the page's box and type from the standard
settings, its layout key), `ReadingFaces` (the faces with the book's own
fonts and small capitals), `word_boxes`, `draw_lines` and `draw_image`;
`reading::note_text` is the text a footnote shows. The layout keeps at
least `Style::widows` lines of a paragraph at the top of a page and
`orphans` at the bottom (`Style::lines_kept`, from the settings
`WIDOWS` and `ORPHANS`; a paragraph that goes on whole takes its
heading along), and
`layout::wrap_lines_kerned` breaks lines of one's own as pages do, with
the pairs `Typesetter::kern` gives (`ReadingFaces` kerns as it draws;
`FontHandle::kern`, `unkerned` and `ligatures` for text of one's own).
For a custom balanced layout,
`paperpaw_text::wrap_lines_balanced_kerned_with_cost` takes an
additional cost callback `(unused_width, spaces) -> f32` for non-final
candidate lines. For example, `|free, spaces| free * free / (spaces.max(1)
as f32)` adds a penalty for concentrating spare width in few gaps. The
ordinary raggedness and hyphen costs still apply; the existing balanced
functions keep their behavior. Readers using `PageType` and
`layout_page` already get bounded justification and matching break
selection. Their page indexes must use the current layout key;
`layout::VERSION` is a cache revision, separate from the native plugin
interface.
`PageType::patterns` gives the patterns of the page's language with the
letters the reader keeps before and after a hyphen (`HYPHEN_MIN_LEFT`,
`HYPHEN_MIN_RIGHT`; 0: the table's own, see
`hyphen::Patterns::with_minimums`); a reader of one's own hyphenates
with them.

A text filter that needs to know where a block is, or leaves blocks out
(a web page's adverts, printed page numbers in a book), is a block
filter: `TextFilterSpec::block` gets a `FilterContext` (the book, its
language, the place) and answers `Verdict::Keep` or `Verdict::Drop`.

### What every library offers

A view of the library of one's own (bound to `core.library`) is as
fast as the shipped one, on the X4 too, as both ask the same service
and read the same catalog; plugins that add to the library add to
every view of it.

| What | Standard | |
|---|---|---|
| The library to ask | `library::service(env)`, a `trait Library` | A book's details, the catalog's state, queries, the index of all books, finished and favourite. Deleting books: `delete`, `can_delete`. A library of one's own provides a `LibraryService` as `library::service::SERVICE` in `library::service::VERSION`, with at least `each`, `records` and `version`. |
| Every book, without reading each | `library::catalog` | One line a book: author, title, series, number, tags, how far read, favourite, opened, added, fields. Read in pieces with `each`, `each_at` and `read_at`. |
| Books found, a page at a time | `catalog::Query`, `query`, `found` | by how far read, words, author, series, tag, folder, added since; sorted by title, author, series or added |
| All books to sort and filter in memory | `library::index::Index` | 24 bytes a book, at most 2000 on the X4 (48 KB). Kept in a file while a book is open with `save` and `load`. Status changes are taken with `refresh`. |
| The catalog made anew | the kernel, as work in the background, never while a book is open | Walked once a view asks after a start (`library::request_catalog`, `request_catalog_anew`). Also for a new library folder and after `LIBRARY_CHANGED`. Caught up after `BOOK_ADDED`, `BOOK_MOVED` and changed fields. `BOOK_REMOVED` leaves a book out at once. Then `events::CATALOG_CHANGED` comes. |
| Something of one book changed | `events::BOOK_CHANGED` (`library::changed_book`, bits of `library::what`) | finished, a favourite, read further, details, cover, a field |
| A book came | `events::BOOK_ADDED` (`library::added(path, source)`) | transfers, syncs and downloads send it |
| A book deleted | `Library::delete`, which answers `library::Deleted` | Only books are deleted: a format known by its extension or first bytes, outside PaperPaw's own folder (`library::is_book_path`). Others are refused, and what fails says why. Asking first is the view's job, as the book's sheet does. The kernel's library removes the files and sends `events::BOOK_REMOVED` for each. The kernel then forgets the book and removes what is kept for it in the folders of `Registrar::add_book_data`. Views leave it out at once. |
| What can be done with a book | `menus::BOOK`, `events::BOOK_CHOSEN` | add an action to the menu, hear which book it was chosen for |
| Another way to open a book | `menus::OPEN_WITH`, `events::BOOK_CHOSEN` | add an action to the list of *Open with …*, beside the reading apps; it hears the book as the book menu's actions do |
| A book's information | `slots::BOOK_INFO` | lists the fields of `add_book_field` |
| Where the books are | `settings::LIBRARY_FOLDER` (`library::folder(env)`) | the kernel's setting, chosen with `screens::FOLDER_EDITOR` (the folder chooser of `slots::CHOOSE_FOLDER`) |

How far a book is read and whether it is a favourite change while the
catalog stands: the kernel notes such changes beside it (when a book is
opened, closed or finished, and through the service's setters), so a
view knows them at once without a new catalog and without reading each
book. A view keeps nothing of the catalog but its index, of the
library's version (`Library::version`: keys hold only for it,
`Library::is_current`); a page's books are read by their keys in one go
(`Library::records`), their badges asked then
(`library::service::badges`), never while drawing.


### Services and points of your own

A plugin can offer others more than the kernel knows of. A service is
a value provided under an id in its namespace, in a version, usually a
struct holding a trait object whose trait the plugin's crate exports.
Its type names its contract in a key, such as `"weather.forecast/1"`,
written with `service_type!`; the key changes (`/2`) when the type's
fields do. Others use the service when it is there and do without it
when not:

```rust
// In the providing plugin's crate.
pub struct Weather(pub Box<dyn Forecast>);
paperpaw_sdk::service_type!(Weather, "weather.forecast/1");

// The providing plugin's `register`.
r.provide(WEATHER, Weather(Box::new(Today::new())), Version::new(1, 0, 0))?;

// Any other plugin, wherever it has an `Env`: version 1.0 or later.
if let Some(weather) = env.services().get::<Weather>(WEATHER.as_str(), Version::new(1, 0, 0)) {
    let today = weather.0.today();
}
```

A service is found by its key, so native code plugins built apart from
the firmware find it too (see *Plugins as files* below); its size and
alignment are checked.

A point is an id a plugin names, with what a contribution there is in
its documentation (for example one line per entry); others add to it
with `r.contribute(point, text)`, and the plugin reads
`env.services().contributions(point)`, each with the plugin it came from.

### Marks (decorations)

A plugin marks words on book pages with a decorator (ADR 0021): it gets
the words of a page with their places in the book and gives back ranges,
each with a *role*. The reader draws them; the plugin knows neither the
reader nor the panel.

```rust
use paperpaw_sdk::decorations::{self, Decorated, Decoration, DecoratorSpec, LineKind};

const LEARNING: Id = Id::new_static("vocabulary.learning");

// The role: its look is settings (Settings, Marks), the
// plugin gives the default; themes and users change it.
decorations::add_role(r, &LEARNING, "vocabulary.role.learning",
    Decoration::NONE.line(LineKind::Dotted))?;
r.add_decorator(
    DecoratorSpec::new(Id::new_static("vocabulary.words"), "vocabulary.decorator",
        |env, page| {
            page.words
                .iter()
                .filter(|w| is_learnt(env, w.text))
                .filter_map(|w| Decorated::words(core::slice::from_ref(w), &LEARNING))
                .collect()
        })
    .switch(&MARK)          // a bool setting turns it on and off
    .taps()                 // taps on its marks come back
    .signature(|env| words_changed(env)),
)?;
r.on(events::DECORATION_TAPPED, |env, tapped, out| { /* tapped.text */ })?;
```

- A `Decoration` is a background, the text's ink, a line under the text
  (solid, dotted, dashed, wavy, double) with its tone and thickness, a
  frame, struck through; tones are named, and each panel shows them as
  well as it can (colours, or their lightness, dithered where there are
  few grays).
- Each aspect of a role is a setting of its own (`<role>.background`,
  `.ink`, `.line`, `.line-tone`, `.thickness`, `.frame`, `.struck`), so
  profiles, rules and themes set them like any other.
- Decorators are asked once a page is laid out, and again when their
  signature changes (their data, their settings): keep it cheap, and
  never work on a whole book.
- Where marks meet on a word, the earlier decorator (by `order`) wins an
  aspect both set; the rest combine.
- The reader's own marks are roles too: `roles::HIGHLIGHT`, `LINK`,
  `SEARCH_HIT`, `ALOUD`. A reader of one's own draws all marks with
  `decorations::draw_behind` and `draw_over`.
- What changes widths (bold, a larger size) is a text filter instead: it
  lays the page out again.

Words found by patterns need no code of one's own:
`decorations::PatternDecorator` marks what its patterns
(`paperpaw_sdk::pattern`, a small part of regular expressions: classes,
`\b`, repeats, `|`, groups, `(?i)`, with bounded work) find on the page's
text, each in its role:

```rust
let names = PatternDecorator::new()
    .rule(Pattern::new(r"\b(?:Mr|Mrs|Miss)\. [A-Z]\w+").unwrap(), NAME)
    .into_spec(Id::new_static("tale.names"), "tale.names");
r.add_decorator(names)?;
```

A plugin without code does the same with a `marks.ini`, a section per
role (`<plugin>.<section>`), its patterns and its look:

```ini
[names]
title = Names
pattern = \b(?:Mr|Mrs|Miss)\. [A-Z]\w+
background = yellow
line = wavy
```

Any plugin can read the words of the page shown, not only decorators:
`reading::ask_page_words(&MY_WORDS)` asks the reader, which answers with
the event `MY_WORDS`, a `reading::PageWords` (the book, the page's start
and end, its words with their places).

### Drawing on the page

A page layer (`layers::PageLayerSpec`) draws on every page a reader
shows: side notes, a reading ruler, change bars. It may reserve room at
a side of the text, which is then laid out without it, and it hears
taps on that room (`events::PAGE_LAYER_TAPPED`, also when the room is
chosen with the buttons, as links are):

```rust
use paperpaw_sdk::layers::{PageLayerSpec, Side};

r.add_page_layer(
    PageLayerSpec::new(Id::new_static("notes.margin"), "notes.margin", |canvas, env, page| {
        let Some(room) = page.room else { return };
        for line in page.lines.iter().filter(|l| has_note(env, page.path, l.start)) {
            canvas.fill_rect(room.x.into(), line.area.y.into(), 4, line.area.height.into(), Color::BLACK);
        }
    })
    .reserve(|env| Some((Side::Right, env.metrics().mm(8) as u16)))
    .taps(),
)?;
```

- `page` (`PageView`) has the book, the page's start and end, the
  text's box and columns, its lines (`LineBox`: where each is and what
  it holds), the status lines and the layer's room.
- Layers draw what they know from memory, on some devices once for every
  band of the page: they read nothing from storage there. Those of an
  order below 0 are drawn behind the text.
- A layer whose drawing changes while the page stays asks for the
  screen to be drawn again (`Command::Redraw(RefreshMode::Fast)`).

A layer that writes between the lines, an interlinear reading with a
small translation above each line, takes two parts: a text filter asks
for room above every line (`Block::leading_above`, in hundredths of an
em), which the layout adds to each line's height, so pages break with
it and words are hit where they are drawn; and a layer that asks for the
page's words (`.words()`) finds them in `page.words` (`WordBox`: each
word's box, text and place), measured once a page and only where a
layer asks:

```rust
const ON: Setting<bool> = Setting::new("interlinear.on");

r.add_text_filter(TextFilterSpec::new(
    Id::new_static("interlinear.room"),
    "interlinear.room",
    // Seven tenths of an em above every line, while switched on.
    Box::new(|env, block| {
        if env.settings().read(&ON) {
            block.leading_above = Some(70);
        }
    }),
    // Changes with the setting: pages are laid out anew.
    Box::new(|env| u64::from(env.settings().read(&ON))),
))?;
r.add_page_layer(
    PageLayerSpec::new(Id::new_static("interlinear.words"), "interlinear.words", |canvas, env, page| {
        for word in page.words {
            // The room above the word's line: line.leading pixels high.
            let Some(line) = page.lines.iter().find(|l| l.start <= word.start && word.start < l.end) else { continue };
            let gloss = translation(env, page.language, word.lookup_text());
            draw_small(canvas, env, word.area.x, line.area.y - line.leading, gloss);
        }
    })
    .switch(&ON)
    .words(),
)?;
```

A filter may also insert blocks after the one it gets: a translation
after each paragraph (a parallel text), a gloss, a note. They show after
the block and never apart from its last line, and take no place in the
book: positions, progress, bookmarks, highlights and search stay the
book's. At most four, 4 KiB of text, and best shorter than a page (they
cannot go on over a page break):

```rust
let translation = translate(&block.text);
let len = translation.len() as u32;
block.insert_after(Block::new(BlockKind::Paragraph, 0, len, translation));
```

A layer takes taps on what it draws by saying where its items are; a tap
on one, or Select on one chosen with the buttons as links are, comes to
its plugin as `events::PAGE_LAYER_TAPPED` with the item's id
(`tapped.item`), and every other tap and button stays the reader's:

```rust
PageLayerSpec::new(id, "interlinear.words", draw)
    .items(|_, page| {
        page.lines.iter().enumerate()
            .filter(|(_, l)| l.leading > 0)
            .map(|(i, l)| LayerItem::new(i as u32, Rect::new(l.area.x, l.area.y - l.leading, l.area.width, l.leading)).label("Translation"))
            .collect()
    })
```

`samples/interlinear` is all of this as a plugin to start from.

A frame (`layers::PageFrameSpec`) sets the text in columns: two
columns, two pages side by side. The setting `core.page-frame` (each
book may have its own) offers every frame by its id, named by the
translation of the id:

```rust
r.add_page_frame(PageFrameSpec::columns(Id::new_static("columns.two"), 2, 24))?;
```

### Pictures over what is drawn

Any screen, widget, theme or overlay draws a picture over what is drawn
already: a PNG or GIF keeps its transparency (an alpha channel, a
palette's or a colour key) when decoded with
`image::picture_with_alpha` (others come out opaque; why not, where it
cannot be read), and `ui::draw_image_over` blends it in, its clear
parts showing what is below, its edges mixed with it, and all of it as
see-through as its `opacity` (0 to 255) says. The mix is dithered as
pictures are, so it keeps its lightness on panels of two or four grays;
rows are read and blended one at a time, so a picture takes no memory
of its size. Pictures made larger than they are decoded are
interpolated, not blocky.

```rust
match image::picture_with_alpha(env.storage(), "plugins/owls/owl.png", &cache, Fit::side(200)) {
    Ok(image) => ui::draw_image_over(canvas, env.storage(), &image, area, 192), // a quarter see-through
    Err(why) => { /* e.g. "unsupported image: interlaced PNG" */ }
}
```

An overlay that draws only part of its region, or blends over it, says
what shows through it (`Screen::shows_through`): every screen below it,
menus too (`ShowsThrough::All`), or the topmost full screen alone, a
menu left open over it not drawn (`ShowsThrough::FullScreen`); what
shows is drawn first under all of it. A screen guarding the device is
marked in `env.stack()` (`StackEntry::guards`), so that nothing shows what
is below it. The sleep screen's *The page with a picture* shows the
full screen through: a picture or a widget over the page being read;
pictures for it are offered as presets (below).

A setting's choices are open to other plugins: `Registrar::add_choice`
offers a value of one's own among them (in one's namespace, the
translation key of its name), after the setting's own. This is how the
sleep screen shows a screen of another plugin as one of its modes. The
flash card due and the reading cat are such modes:

```rust
r.add_screen(ScreenSpec::new(Id::new_static("aquarium.sleep"), "aquarium.sleep", factory))?;
r.add_choice(Id::new_static("sleep.show"), "aquarium.sleep")?;
```

### Widgets

A widget is a [`Widget`] a screen makes and keeps
(`env.widgets().make`): it reads what it shows in `update`, which the
screen calls when it shows the widget, about each minute and after
events, and only draws in `draw` (which runs for every band of a
frame). The screen tells it where it goes (`place`) before drawing it
there: what is made for that size, such as a cover made small
(`ui::Thumbnail`), is made then, and again in `update` when what it
shows changed. Register it with `WidgetSpec::new(id, title, factory)`; a widget
with nothing to read, such as a clock, is a pair of functions
(`WidgetSpec::drawn` with `Drawn::new(height, draw)`, and `shows` to be
drawn again when what it shows changes). A widget whose content changes
only after events (books, settings) says so (`live` false) and is not
read again each minute; one that can be drawn smaller says how small
(`min_height`), so that a grid fits it where its full height has no
room. `widgets::grid` puts widgets
and tools in a grid as the home screen does, edited in place
(`GridView`). Its screen plate follows each widget's corners and leaves
room for its shadow.
Widgets with zero content height have no plate outside editing mode. Frameless
widget paper follows chosen corners; Automatic keeps it square.
Say which kind a widget is
(`.category(widgets::categories::TIME)`, or `BOOKS`, `READING`, `WORDS`,
`PERSONAL`, `WORLD`, `DEVICE`): the home screen offers widgets by their
kind, each drawn as it would show.

A widget may also react. `targets` names the places in it that do
something. `on_input` gets taps, swipes and the buttons there, and
`context` tells it its placement and the screen it is on. `draw_focus`
draws the focus its own way.

### Widgets with settings

A widget may have settings of its own, such as where its text sits. Put
them in a settings group with the widget's id
(`widgets::settings_group`); the settings list leaves such groups out,
and the home screen's editor offers them as the widget's *Settings*. For
the alignment there is a ready setting: register
`widgets::align_setting(&id, Align::Left)` and place each text with
`widgets::align(env, id).x(area, width)`.

Widgets of all plugins share a look the reader shapes for each, so that
they sit well together on a page: give yours a frame and draw in it.

```rust
const ID: Id = Id::new_static("my.card");
static SETTINGS: [SettingSpec; 4] = [
    widgets::align_spec("my.card.align", ID, Align::Left),
    widgets::frame_spec("my.card.frame", ID, Frame::None), // none, line, light or dark card
    widgets::size_spec("my.card.size", ID, TextSize::Medium, true), // true: also "as large as it fits"
    widgets::part_spec("my.card.author", ID, "my.show-author", true),
];
r.add_setting_group(widgets::settings_group(&ID, "my.card"))?;
r.add_settings(&SETTINGS)?;

fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
    let rest = widgets::draw_caption(canvas, env, ID.as_str(), &look, "Today");
    let size = widgets::size(env, ID.as_str()).text(&UiStyle::new(env));
    // ... the content in `rest`, in `look.ink` (and `look.faint` for
    // what matters less), never in a fixed black: on a dark card it is
    // light.
}
```

A widget may be placed more than once, each placement with its own
values of the widget's settings: the home screen places a clock as
`widgets.clock@1` and `widgets.clock@2`, whose frames are the settings
`widgets.clock@1.frame` and `widgets.clock@2.frame` (saved like any
other, the widget's own `widgets.clock.frame` where one sets none).
`env.widgets().make` gives a placement's widget, which reads its settings
by their usual ids and gets that placement's values
(`Settings::with_placement`); a widget need do nothing for it. Screens
of their own that place widgets use `Id::placement` and forget a
placement's values when it goes (`Settings::placement_values`).

`widgets::shows(env, id, "author")` reads a part's switch,
`widgets::choice_setting` makes other choices of the widget's own, and
`widgets::draw_message` says kindly what is not there yet. A widget's
`height` counts its frame (`widgets::widget_look` gives the area inside
it without drawing). For round things there are `Canvas::fill_circle`,
`stroke_circle`, `fill_ring` (arcs, e.g. a progress ring) and
`draw_hand`, with angles as on a clock face (`paperpaw_sdk::round`);
light grays are drawn with `fill_shaded` or `fill_round_shaded`, which
dither them on black and white panels.

Screens that place widgets one below the other should know the kernel's
two widgets for room between them: `widgets::SPACE` is an ordinary
widget, a gap; `widgets::FILL` has no height of its own and takes its
share of the room the others leave (`widgets::fill_height`). Both may
be in a list more than once (`widgets::repeatable`).

### Presets

A preset is a ready-made set of setting values chosen as a whole
(`paperpaw_sdk::presets`). It has a kind, named by the plugin that
offers such presets. That plugin lists those of its kind with
`env.settings().presets().of_kind(kind)`. It works out what one changes
with `Preset::changes` and the settings it covers. It shows it with
`Command::PreviewSettings`: the values show on every screen without
being kept, until the screen that asked closes. It keeps it with
`presets::commands`. The settings of its kind that a preset leaves out
go back to their defaults.

The kinds PaperPaw's own plugins offer are themes (`core.theme`,
below) and the power modes, `core.power-mode`, which the kernel offers.
The sleep screen offers pictures over the page as `sleep.overlay`, see
[Plugins without code](#plugins-without-code). Add presets kept in the
program with `Registrar::add_presets`, a `static` list of
`Preset::fixed`. They may be of your own kind or another plugin's:

```rust
static THEMES: [Preset; 1] = [Preset::fixed(
    "my.dusk",
    presets::THEME,
    "my.dusk",      // translation keys of its name
    "my.dusk.help", // and of a sentence on it
    "core.ui-focus = core.focus.ring
core.ui-separators = core.separators.none",
)];

r.add_presets(&THEMES)?;
```

A theme is a preset of the kind `core.theme` (`presets::THEME`): values
for the appearance settings. Where any plugin brings one, the
appearance settings begin with *Theme*, which lists them; the one
chosen gives the appearance settings its values and those it leaves
out their defaults, so two themes installed never fight. A data plugin
brings themes in its `presets.ini`:

```ini
[paw]
kind = core.theme
name = Paw
description = Round and friendly: a frame for the focus, no lines.
core.ui-corners = core.corners.soft
core.ui-focus = core.focus.ring
core.ui-separators = core.separators.none
```

### Hints for a screen

The first time a screen opens, a short note over it says what its
controls do: the hints its plugin gives with `help::add`, each a text
and, where it is about a part of the screen, that part (in thousandths
of the screen's width and height), framed. A hint is for touch, for
buttons or for both; a device shows those that fit its input. A tap or
any button closes the note, and it does not come again for that screen;
*No more hints* and the settings switch them all off.

```rust
use paperpaw_sdk::help::{self, HelpHint, Zone};

help::add(r, SCREEN.as_str(), &[
    HelpHint::touch("hello.help.tap").zone(Zone::new(0, 0, 1000, 120)),
    HelpHint::buttons("hello.help.confirm"),
    HelpHint::any("hello.help.back"),
])?;
```

The texts are keys of the plugin's translations; the screen is any
screen's id, so a plugin can add hints to another's. A screen that first
prepares what it shows (the reader, opening a book) has its hints wait
with `help::wait(r, SCREEN.as_str())` and says when it is ready,
`cx.command(help::ready(&SCREEN))`: screens below another get no timers,
so it would not get ready under them. The note is the
`help` plugin's; another plugin can show them its own way, from
`help::hints(env, screen)` when `events::SCREEN_SHOWN` comes.

### Passwords and keys

Plugins keep passwords, API keys and tokens with
`Command::set_secret(SecretChange::keep(id, label, value))` and read them
with `env.secrets().get(id)`. They are all listed in *Settings, Passwords
and keys*, where the user changes or removes them. Ask for one on the
keyboard in password mode (`ask::text(…).password()`); the keyboard
offers to type it on a phone. Ask for a message or a note of several
lines with `.multiline()`: the keyboard gets a key for the line break, shows
the text on as many wrapped lines as fit above the keys, and the answer
has `\n` between its lines. Ask for web and e-mail addresses with
`.url()` and `.email()`: the keyboard types them as they are, without
capitals of its own. They are kept apart from the settings and
not readable at a glance, but not encrypted: the devices have no safe
place for a key. A plugin sees its own alone: name them in its
namespace (`myplugin.token`); `env.secrets().get` gives another plugin's
none, and `env.secrets().listed()` names every secret without its value.
A setting whose extra is `SettingExtra::secret()` is kept the same way. Native code is not
sandboxed and could go past this; the SDK hands no plugin another's
secret.

### The network

`Command::http(HttpRequest::get(url, &FETCHED))` makes a request where
the device has a network (`env.caps().network`); the answer comes as the
request's reply event, an `HttpReply` with the status and the body, or
saved to a file (`.save_to(path)`) for large ones. A request may set a
`timeout`, the answer's headers it wants back (`want_headers`) and the
longest body kept in memory (`max_body`, by default 64 KiB on small
devices). Every request is answered once, also when it is refused
(its `refused` says why and when to try again). The manifest says
that the plugin uses the network (`.uses(Uses::NETWORK)` in its manifest); without it,
requests are refused. In Offline (`env.power()`), only what the reader went
online for goes out: a screen that needs the network says so on itself
and offers *Go online*, which sends `Command::GoOnline` (the network for
that screen until it closes); anything else is refused with
`Reason::Offline` and may try again on `events::WORK_ALLOWED`.
`Command::CancelHttp` takes requests back. A download that may take a
while says how far it came with `.progress(&PROGRESS)`, an
`Event<HttpProgress>`: the bytes received, the size where the server
said (`permille()` for a bar), and the request's parameters, at most
every 5 % or every 2 seconds and never after the reply, so a screen
redraws its bar no more often (only the bar's area, with a partial
refresh). Downloaded into a file of the plugin's own and moved into the
library once whole, a download taken back leaves nothing there (as the
OPDS plugin does). Where a server or a computer
is told which device this is (a sync service, Calibre), the name is
`env.device_name()`: the one the reader set under *Device name*, else
the model's; a plugin never makes one of its own.

### Bluetooth

Where the device has a Bluetooth Low Energy radio the kernel uses
(`env.caps().bluetooth`; `env.bluetooth()` shows the radio's state and
the bonded devices given to the plugin), a plugin that says so in its
manifest (`.uses(Uses::BLUETOOTH)`) sends commands written with
`paperpaw_sdk::bluetooth` and hears everything about them as its own
events, each a `BtEvent` (ADR 0027). Every command is answered: by its
result, or by a `BtEvent::Failed(BtFailure)` that says which command
(`BtOp`) failed for which device and why (`BtError`: `Off`,
`NotAllowed`, `NotFound`, `Busy`, `Timeout`, `Declined`, `Lost`,
`Att(code)`, `Held(refusal)`, `Unsupported`).

Bluetooth costs battery, so everything is cheap unless asked otherwise.
The kernel switches the radio on for the first request and off a little
after the last; scans and advertising always end after a while; links
run at long intervals; and scans, links and advertising end with the
screen that asked for them (or with declared work, `Work::bluetooth`),
and all of them before the device sleeps. Where plugins ask for
different powers, the radio runs at the cheapest one that serves them
all, and the time it was on is counted (`Usage::bluetooth_ms`).

- **Scanning**: `bluetooth::scan(&FOUND).service(Uuid::sig(0x180D))
  .command()` finds devices in reach (`BtEvent::Found` with a `Device`:
  its id, name, signal, services, advertised data and kind), then
  `BtEvent::ScanEnded`. A scan lasts 10 seconds unless
  `.lasting(duration)` says otherwise (a minute at most), listens
  passively and seldom (`ScanPower::Low`; `.power(ScanPower::Balanced)`,
  `.active()` to ask devices for their names), and `.first()` ends it
  with the first device. `ScanPower::Fast` is only for the reader's own
  doing, and a plugin scans again only a minute after its last scan
  ended, unless the reader asked. A plugin hears only what passes its
  own filters.
- **A GATT client**: `connect(device, &LINK).command()` (`.power(...)`
  for shorter intervals, `.idle(duration)` to let go of the link when
  nothing was asked for that long), then `discover(device).command()`
  lists the services and their characteristics (`BtEvent::Discovered`;
  `bluetooth::characteristic(&services, service, characteristic)` finds
  one), `read`, `write(device, handle, bytes)` (`.without_response()`),
  `subscribe` to notifications (`BtEvent::Value`), or
  `subscribe_to(device, service, characteristic)`, the handle found by
  the kernel; `disconnect`.
- **Pairing**: `pair(device)` asks the reader, in the kernel's own
  question, never the plugin's; the device is then bonded and given to
  the plugin (`BtEvent::Paired`). A plugin sees only devices its scans
  found and bonded devices the reader gave it; anything else fails with
  `BtError::NotFound`. Devices are known by a `DeviceId`, never by their
  address. The Bluetooth manager, the plugin whose screen is bound to
  `slots::BLUETOOTH`, sees every bond; `forget`, `give` and `take_back`
  count otherwise only as the reader's own doing.
- **Keys of a remote or a keyboard** need no plugin: once the reader lets
  a bonded device type (`remote_keys(device, &DONE)`), its keys are
  buttons of the key maps, `button.hid.right`, `button.hid.page-down`,
  `button.hid.volume-up` and so on, bound like the device's own. Its
  link slows down a minute after the last key. A plugin the device is
  given to may ask for the reports themselves with
  `hid_reports(device, &REPORTS)`.
- **Being seen**: `advertise(&SEEN).name("…").service(uuid).command()`,
  for a minute unless `.lasting(…)` says otherwise (ten minutes at
  most), and services of the device's own GATT server declared with
  `Registrar::add_gatt_service(ServiceSpec::new(uuid, &EVENTS)
  .characteristic(CharacteristicSpec::new(uuid).read().notify()))`:
  centrals' reads (`BtEvent::ReadRequest`, answered with `respond` or
  `refuse`), writes and subscriptions come as events, and
  `notify_centrals` sends to those subscribed. Characteristics need an
  encrypted link unless `.access(Access::Open)` says otherwise.

Events written out for others (observers, rules) carry no payloads:
neither readings nor keys nor what a phone sent. A heart-rate reader:

```rust
const FOUND: Event<BtEvent> = Event::new("pulse.found");
const LINK: Event<BtEvent> = Event::new("pulse.link");
const HEART_RATE: Uuid = Uuid::sig(0x180D);
const MEASUREMENT: Uuid = Uuid::sig(0x2A37);

r.add_action(ActionSpec::new(START, "pulse.start").handler(|_, _, out| {
    out.push(bluetooth::scan(&FOUND).service(HEART_RATE).first().command());
}))?;
r.on(FOUND, |_, event: &BtEvent, out| {
    if let BtEvent::Found(device) = event {
        out.push(bluetooth::connect(device.id, &LINK).command());
    }
})?;
r.on(LINK, |_, event: &BtEvent, out| match event {
    BtEvent::Connected { device, .. } => {
        out.push(bluetooth::subscribe_to(*device, HEART_RATE, MEASUREMENT));
    }
    BtEvent::Value { bytes, .. } => {
        // Flags first; the rate is one byte, or two where flag 0 is set.
        let bpm = match bytes.as_slice() {
            [flags, low, high, ..] if flags & 1 == 1 => u16::from_le_bytes([*low, *high]),
            [_, rate, ..] => u16::from(*rate),
            _ => return,
        };
        out.push(Command::set(&BPM, i64::from(bpm)));
    }
    _ => {}
})?;
```

The emulator pretends to have Bluetooth for the X4 Pro's profile, with
made-up devices in reach: `PaperPaw Remote`, a page-turner whose keys a
script presses with `ble-key:right` (or `left`, `page-down`,
`volume-up` …) once they go to the key maps, and `Pulse HR`, a
heart-rate belt that answers the example above, notifying a rate each
second.

### Looking different: tone, look, hooks

Three ways to change how things look, from the least to the most code. Each
changes how a thing *looks*, never where it goes or how wide it is.

- **Numbers (no code).** A theme or a reader sets `core.text-curve` (book
  text; in place of *Text contrast* when set) or `core.ui-text-curve` (the
  interface) to a list of `[script=]curve` entries separated by `;`, e.g.
  `smooth; arab=contrast:1`. A curve is `smooth`, `contrast:-1` to
  `contrast:2`, `sharp:128` or 16 numbers from 0 to 255 (the opacity of
  each of the 16 coverage levels of a letter's edge). On a black-and-white
  panel the curve decides which edge pixels turn black, so it is also the
  knob for thicker or thinner letters. In code: `ToneCurve::parse_for`,
  `FontHandle::inked(TextInk::new(curve))`.
- **A look.** `Painters::text` paints the text that `ui::Label` draws (a
  shadow, an outline): draw with `part.font.draw(..)` and return `true`; the
  width stays the stock one. `Painters::extras` takes a `static` list of
  `ExtraPaint`s for parts of kinds that come later (scrollbars, tabs): a
  look made now works when the SDK learns to ask
  (`Capability::LOOK_EXTRAS`). The SDK's own rows, headers,
  buttons, hints and widgets' captions pass it too, with their role
  (`TextRole::BUTTON`, `HINT`, …); text a plugin draws itself does so with
  `ui::draw_label_in(canvas, font, style, role, ..)`. `ui::draw_label` is the
  stock look.
- **A render hook.** `Registrar::add_render_hook` with a `RenderHookSpec`
  changes each finished band (night or sepia tint, grain, a better dither of
  the four-gray pass). **Check `Capability::RENDER_HOOK`:** without it the
  hook is accepted and never run (today's firmware). A hook sees pixels, not
  coverage, so it cannot make letter edges softer (use the tone curve for
  that); it must give the same rows for the same band and carry (no clock or
  random: `HookCx` has none); state across bands goes in `BandView::carry`,
  which the kernel zeroes at the start of each pass; a panic aborts the
  device.

**Colour roles.** `style.ink(Ink::MUTED)` gives the colour of a role of the
interface; a theme sets them in `core.ui-palette` (`muted=60 link=#2a6f97`,
a gray or `#rrggbb`; a gray panel uses the brightness). `Ink` is a number
with constants, so roles can be added: `ACCENT`, `ON_ACCENT`, `LINK`,
`SELECTION` and `WARNING` exist (14 to 18, 19 to 31 are free). A role a theme
does not set reads the colour of `Ink::fallback()` (`LINK` reads `TEXT`), so
draw with the role that says what a thing *is* and a theme made for fewer
roles still looks right; a role you do not know the name of also works.
Do not mix this up with `TextInk`: that is *how text is inked* (its tone), an
`Ink` is a colour role.

`Canvas::pixel` and `gray_at` read back what *this band* drew, to composite
pixels you drew, never to decide what to draw (a point in another band is
`None`, and decisions would show seams).

### Screens that run in real time

Games, emulators and animations draw many frames a second and want
every button as it goes down. Such a screen says so in one place,
[`Screen::mode`], with a `realtime::ScreenMode`
([ADR 0030](adr/0030-real-time-screens.md)):

```rust
fn mode(&self) -> ScreenMode {
    ScreenMode::new()
        .full_screen()     // no row of hints, no bars
        .raw_keys()        // buttons pressed and released, in on_key
        .cleans_itself()   // no full refresh every few pages
        .fastest()         // the fastest refresh the panel has
        .grays(2)          // black and white is enough
        .frame_every(Duration::from_millis(100))
        .awake_for(Duration::from_secs(600))
}
```

Each option asks for an intent; every device meets it as well as it
can. With `raw_keys`, `on_key` gets each button pressed and let go,
with its time, and `env.keys_held()` says which are down. The power
button stays the device's, and holding *Back* for two seconds always
leaves the screen. With `frame_every`, the screen asks for its timer as
usual (`cx.wake_at`); the X4 then goes on while the panel refreshes, and
shows the next frame as soon as the panel is free.

The device says what it can do and what it did: `env.panel()` has the
fastest time of each refresh mode as measured, its grays, and the last
frame shown (`last_frame()`: when, how long it took, how many frames
were merged into it). Pace yourself by these numbers rather than by
your own: faster panels then make your plugin faster without a new
build.

Draw pictures that change every frame with `canvas.blit`: a packed
`Bitmap` of 1, 2, 4 or 8 bits a pixel with a palette of grays, at a
whole scale, dithered or not. A 160×144 picture at three times its size
costs a fraction of drawing it pixel by pixel.

Keep what a loop reads all the time in internal RAM and large data in
PSRAM (`memory::vec_in(Placement::FAST, n)` and `Placement::LARGE`);
devices with one kind of memory keep both alike. In a code plugin, mark
the few functions that run every frame with `paperpaw_sdk::hot! { … }`:
the X4 runs them from internal RAM instead of flash, a bounded part of
its heap; what does not fit runs from flash as before.

A screen that needs much memory says so (`.memory(bytes)`, and
`.needs_network()` or `.needs_bluetooth()` where it does): as it shows,
the kernel lets caches go, holds background work back, switches off
the radios it does not need, asks the screens below to let go of what
they can make again (`on_squeeze`; the reader lets its book go and
keeps its place) and closes them where that is not enough, until there
is enough, and gives everything back once it closes. Read what you got
with `env.room()` in `on_show`, and make the large buffer there. A
screen of yours that keeps much (pictures, a long list) lets it go in
`on_squeeze` and makes it again in `on_show`.

To keep the device from going to sleep by itself for a while (a clock,
a slideshow, a long download), hold it: `cx.keep_awake(WakeHold::new(id,
reason).lasting(...))` on a screen (it ends when the screen closes), or
`Command::KeepAwake` from work and services; renew it before it ends,
let go with `Command::LetSleep`. The reader can switch holds off.

`samples/frame-test` does all of this and shows what it measures (frames
a second, the work as hot and as ordinary code, the drawing, the heap):
`cargo run -p paperpaw-emulator --features frame-test`, then *Frame
test* in the tools.

### Work in the background

What a plugin does on its own, not because the reader just asked for
it, it declares as work (`paperpaw_sdk::work`, ADR 0025), and the
kernel runs it at the moment that costs least: waking, a book opened or
closed, going to sleep, the radio on anyway, charging, idle.
`Command::work(Work::network(PUSH).within(Duration::from_secs(1800))
.at_moments(Moments::BOOK_CLOSED | Moments::SLEEP))` sends the reading
position when the book is closed or the device sleeps, at the latest
after half an hour; the kernel then emits the work's event and the
plugin makes its request. `optional()` work (weather, headlines) runs
only while that is cheap, and never brings the radio on by itself.
`Work::at(event, time)` is a timer, `every` repeats, `key` tells
several works of one event apart, `Command::CancelWork` takes one back.
`env.reader()` says whether what is handled is the reader's own doing: an
action the reader ran acts at once, the same action run by a rule
declares `Work::when_cheap(id, within)` instead and waits for a good
moment.
Long work in steps (an install, a catalog walked) registers its step
function (`r.add_work(WorkSpec::new(id, title, step))`) and is stepped
between frames. `env.work()` says where a work stands, for the plugin's
own page; background work never shows a notice over reading. The power
mode (`env.power()`) makes deadlines longer or holds network work back;
most plugins need not read it.
Read large JSON answers as they stream with
`paperpaw_sdk::json::stream`, so they need little memory.
`Command::Serve` serves HTTP to the local network, e.g. for transfers
from a computer or typing on a phone; what happens to it comes as
`ServerNotice`s. A plugin answers each request with `Command::Respond`.
A server stops when the screen that started it closes. Serve only while the screen showing the
address is open, and guard it as `paperpaw_sdk::served` does: a key
drawn anew each time in the address (`served::key`), checked by a
`served::Guard`, which turns requests away for a while after a few
wrong keys. Where PaperPaw runs Wi-Fi itself (the X4s),
`paperpaw_sdk::wifi` says the radio's state, and
`WifiCommand::Host` opens a network of the device's own for a phone to
join, as the Wi-Fi transfer does without a router. In the emulator
the X4s start with the radio off, as a device does; with `--wifi
online` (over `--stdio`, `start` with `"wifi": "online"`) they start on
USB, in a network `Workshop Wi-Fi` joined before, and a plugin's
requests go out at once.

Other protocols go over sockets ([ADR 0014](adr/0014-sockets-for-plugins.md)):
`Command::Connect` opens a TCP connection, `Command::SocketSend` sends,
and `Command::SocketRead` asks for what comes next, as text
(`SocketRead::Text`) or as a file of a known length written to storage as
it comes (`SocketRead::File`), so files of any size never need to fit in
memory. `Command::Broadcast` finds services in the local network by a
UDP datagram and their answers. Everything comes back as events
(`SocketNotice`s); a socket closes when the screen
that was open as it opened closes. One that background work opens (its
event's handlers, a job's step) is the run's instead: it closes when the
run ends (the job done or failed, the work run again, taken back or
refused, or its plugin gone), whatever screens open and close meanwhile.
One connected from a socket's event is the screens': several connections
in the background are a job's steps. A secure one of background work
ends when idle: `idle_ms(0)` is refused for it (`SocketError::Options`),
at most 30 minutes (`MAX_TLS_IDLE_MS`, as long as IMAP IDLE waits).

A socket may be secure (TLS): `Command::connect_tls(socket, host, 993,
events, Tls::new())` connects and says `SocketNotice::Secured` once the
handshake is done (no `Connected` before). Send and read nothing before:
until `Secured` that fails the socket (`SocketError::NotReady`) and
closes it. For STARTTLS, connect plainly, send the server's command, read
its go-ahead, then `Command::start_tls(socket, Tls::new())`: with a read
under way or bytes come unread it fails and closes, as they would pass as
secure. `Tls::new()` trusts the authorities PaperPaw ships; a server of
one's own with a certificate no authority signed is trusted by
`Tls::pinned_keys(vec![key])` (its key, which a renewed certificate
usually keeps) or `Tls::pinned(vec![hash])` (that certificate alone).
The first connection to it says `SocketNotice::Untrusted` with the
certificate's `CertificateHash` and `KeyHash`: show one to the reader to
compare, and keep it in the plugin's settings once they agree.
`Secured`'s `time_checked` is false where the device did not know the
time yet (an X4 just after a cold start): the certificate's dates were
not checked, so a plugin that must be sure closes and tries again later.
`SocketRead::Bytes` reads what came as bytes (`SocketNotice::Bytes`), for
lines that are no UTF-8. `Capabilities::tls_sessions` says how many TLS connections fit
at once (one on the X4, HTTPS counted): a socket waits for one up to its
`Tls::timeout_ms`, then fails `SocketError::Busy`. A TLS connection holds
about 40 KiB on the X4 and keeps the radio awake, so connect, fetch and
close; `Tls::idle_ms` (60 s) closes a forgotten one.

### Firmware updates

Where PaperPaw is the device's firmware (the X4s), `env.firmware()` says
which version runs and how an install goes; elsewhere it is `None`.
`Command::Firmware(FirmwareCommand::Install { path, key, older })`
installs a signed update file: `key` is the key of one not signed by
PaperPaw that the reader accepted, shown its fingerprint, `older` that
they chose one not newer than the running firmware
(`firmware::Update::may_install` is the shell's check).
`FirmwareCommand::Restart` restarts into it once
`FirmwareState::Installed`; `events::FIRMWARE_CHANGED` comes on the
way. `firmware::Update::read` tells what a file holds before. The
`update` plugin does all this for update files on the card; a plugin
that fetches updates from somewhere else only needs to put the file
there.

`FirmwareCommand::InstallForeign { path }` installs another firmware's
app image instead (the way back to CrossPoint): not signed, so send it
only for a file the reader chose as such, told that it is not PaperPaw.
`firmware::ForeignImage::read` and `fits` tell what the file is before;
the shell checks the whole image as it writes it (`ForeignCheck`: its
chip, segments, checksum and hash, and CrossPoint's board tag).

## Memory

PaperPaw's code guidelines ([CONTRIBUTING.md](../CONTRIBUTING.md#code-guidelines)) hold for plugins too: length limits, errors on user data, memory, comments.

The smallest device, the Xteink X4, has 256 KiB of heap for everything.
The interface-7 SDK handover (`Ui::handover`) allocates nothing: it encodes
at most 32 params and 1,024 content bytes straight into the caller's input
buffer, and parameter text and transport buffers stay borrowed. A unit test
counts its heap use at zero bytes with a counting allocator; the runtime X4
heap gate stays open.

Menu replacement temporarily allocates only its joined item bytes, at most
2,048 bytes, and frees them after encoding into the caller's command buffer,
before host submission. Empty and reset lists allocate nothing. The menu
temporary is released before its host call.

PaperPaw's standard app holds some 90 KiB of it once it has started
with the sample books, 130 KiB at its peak (the emulator, `--profile
xteink-x4 --headless --script wait:500`). A plugin that should run
there:

- keeps little in memory: lists in files, read as they are needed;
- registers what does not change as `static` lists, which stay in flash:
  actions (`FixedAction`, `r.add_actions`), screens (`FixedScreen`,
  `r.add_screens`), menu entries (`r.add_menu_entries`), icons
  (`r.add_icons`) and settings (`r.add_settings`); an `ActionSpec` or a
  `ScreenSpec` is for a handler or factory that keeps state of its own;
- reads files a piece at a time (`Storage::read_at`), never whole books;
- writes downloads straight to storage (`HttpRequest::save_to`);
- reserves large buffers with `try_reserve` and copes when that fails;
- checks `env.device().memory` (`MemoryClass::Small`) to do less there.

The emulator's X4 profile enforces the budget. After a run with
`--headless`, `--heap-report` shows where memory goes.
`--trace-alloc <bytes>` prints each allocation of at least that size.

## Plugins as files

A code plugin can also be a file, `code.ppn`, in a plugin's folder on
the card, which the X4 and the X4 Pro load at their
start (ADR 0024): from the plugin store, from the web flasher with the
firmware, or sent from a computer over the USB cable or the Wi-Fi
transfer, where the device asks first ([USB transfer](usb-transfer.md)).
Only code installed runs. An installer names the file by its digest
(`native::remember_code`, kept in `.paperpaw`, which plugins' folders do
not reach), and a file put into a plugin's folder otherwise is not run;
the plugin manager says why. An author trying their own build sends it
with `tools/usb-transfer.py install <folder>`. Its code is the same as that
of a plugin built in, and it runs as part of PaperPaw, at full speed
and without a sandbox: it can do
whatever PaperPaw can, also past the SDK. The X4 Pro runs it from its
PSRAM; the X4 writes it once into the flash behind its
firmware and runs it from there, not from its RAM.

PaperPaw's own plugins that not every reader needs (statistics, widgets,
Calibre, the syncs and the others of
[`tools/plugin-files/plugins.txt`](../tools/plugin-files/plugins.txt))
come this way on the X4s: their firmware leaves them out, and each
release builds them as files, signed with PaperPaw's plugin key, which
readers install from the store. Each has a version of its own, apart
from PaperPaw's: a release builds one that did not change again for its
firmware, at the version it has, and a fix comes as a newer version,
with a release or alone between releases
([`tools/plugin-files`](../tools/plugin-files/README.md)).

[`samples/hello-native`](../samples/hello-native) is the sample plugin
above as a file, in a workspace of its own as a plugin's own repository
is, with a CI job to start from.

### The crate

The plugin stays a library crate, tried in the emulator and tested as
any other. A second crate, a `#![no_std]`, `#![no_main]` binary, makes
the file with one line:

```rust
#![no_std]
#![no_main]

paperpaw_sdk::plugin!(my_plugin::MyPlugin::default());
```

`plugin!` gives the file its entry, its heap (the kernel's: what the
plugin boxes, the kernel may drop) and its panic handler. The crate's
`.cargo/config.toml` links it as a plugin: copy the sample's, and the
sample's `paperpaw-plugin.x` beside it at the top of the project (the
PaperPaw release's own, from `paperpaw-native`; no build script brings
it, so a build on Windows is the same as one on Linux). It links with
it, keeps the relocations, and on the X4 builds with the medium code model and frame
pointers. It also sets `--cfg paperpaw_plugin`, with which the SDK asks
the firmware for what it has already instead of copying it into the
file (the names of all icons, some 125 KB in a plugin that draws an icon
by name). Build it for both chips:

```sh
cargo x4                  # riscv32imc-unknown-none-elf, the X4
cargo +esp x4pro          # xtensa-esp32s3-none-elf, the X4 Pro (espup's toolchain)
```

The devices draw font packs and load only plugins built without the
SDK's `outline` feature; the emulator's build (for the computer, see
below) takes it. One manifest gives each its own, by target:

```toml
[dependencies]
paperpaw-sdk = "0.6.0-beta.3"

# Outline fonts for the emulator's build only: the X4s draw font packs.
[target.'cfg(not(target_os = "none"))'.dependencies]
paperpaw-sdk = { version = "0.6.0-beta.3", features = ["outline"] }
```

A build for a device that asks for `outline` stops and says so.

The X4's core libraries (`core`, `alloc`) come from rustup, with the
target in the sample's `rust-toolchain.toml`. The X4 Pro's come with each
PaperPaw release, built once and signed
(`paperpaw-xtensa-std-<version>.tar.gz`): install them into the `esp`
toolchain once for each release, with PaperPaw's
`tools/xtensa-std/install.sh` or by hand as its README says. Without
them, `cargo +esp x4pro` finds no `core`.

### Which build fits

A plugin file runs on every PaperPaw of the plugin interface it was
built for ([ADR 0026](adr/0026-plugin-interface-version.md)),
whichever release made it: its fingerprint holds the interface's
version (`native::INTERFACE`, in its top 16 bits), the chip and the
features that change what crosses (`outline`, colour), and a digest of
the layouts of the types that cross. A device loads only files with its
own fingerprint; *About* shows it and the compiler to build with, and
`paperpaw-sign plugin` names a file's interface.

**When to rebuild:** when a release raises the plugin interface (its
release notes say so, and so does the plugin manager on the reader:
*Built for another plugin interface*), and only then. Build with the
toolchain the release names (the sample's `rust-toolchain.toml`, and
espup's of `versions.env` for the X4 Pro): until 1.0 a new compiler is a
new interface. The file names the compiler that built it:
`paperpaw-sign pack` takes it from the plugin's `.comment`, which the
format's linker script keeps, and a device loads the file only where
that compiler built the firmware too. A beta or a release that keeps
the interface runs the plugin as it is. The store and the update check
show which plugins wait for a build. Interface 4 came with 0.5.0-beta.8
(reading, settings and real-time screens), interface 6 with 0.6.0-beta.2
(right-to-left text, shared functions, plugin files of format 4, TLS
sockets, screens that fit another size; interface 5 was never released): a plugin
built for an earlier interface is to be built again for it. Interface 7, the
plugin boundary of 1.0 (ADR 0034), may still change until it is frozen at
1.0: a plugin built from an earlier tree is built again.

### The file in the emulator

The desktop emulator loads a plugin's code built for the computer it runs
on: a `cdylib` crate with the same
`plugin!` line, built with the emulator's toolchain against its SDK with
the `outline` feature (the target section above gives it), is `code-host.dll`, `code-host.so` or `code-host.dylib`
in the plugin's folder (`plugins/<id>/` of the emulator's library,
beside its `plugin.ini`). The emulator runs such code only when it is started
with `--trust-code`, which installs every such file in the library first
(as on a device, only code installed runs); without it, none, whatever
the library says is installed: a library copied from elsewhere runs no
code of its own.

On Linux, the plugin's own `build.rs` must disable the C startup files for
its shared library. Those files add initializers that interface 7's host
plugin preflight refuses. A dependency's build script cannot set this flag
for the plugin:

```rust
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-arg-cdylib=-nostartfiles");
    }
}
```

After building, `readelf -dW libmy_plugin.so` must show no `INIT`, `FINI`,
`INIT_ARRAY` or `FINI_ARRAY` entries. This flag applies only to the plugin's
shared library, not the emulator or its dependencies.

A host plugin must be `no_std` and import nothing from the C library. It reaches
the host only through the host table. Interface 7's preflight refuses a shared
library that names a C library symbol, because that brings a `NEEDED` entry and
symbol-version tables with it; `readelf -dW` must show no `NEEDED`, `VERSYM`
or `VERNEED` entries.

```sh
paperpaw-emu --library my-library --trust-code
```

Over `--stdio`, `start` with `"trust_code": true` does the same, and
`reload` loads a new build; `ready` says the fingerprint and compiler
the emulator takes.

#### Interface-7 libraries (developer switch)

For work on the interface-7 loader, the emulator can load interface-7 host
plugins from the library. It is off by default and independent of
`--trust-code`: start with `--abi7-dev-loader` or `PAPERPAW_ABI7_DEV_LOADER=1`
(Linux x86-64 only). A plugin folder's `code-host.so` is told from its bytes:
`PAPERPAW_METADATA` alone is interface 7, `PAPERPAW_API` alone is
interface 6 and left to the loader above, anything else is refused and
said. Nothing is guessed.

For each interface-7 file the emulator checks the strict freestanding rule
(no `NEEDED`, no undefined symbols, only the two entry exports), opens a
private read-only copy under the id of the plugin's *folder* (never the id the
library reports), and stages what the plugin declares. What is staged is
kept, not applied: nothing a plugin declares reaches the app yet, and no draw
runs, until the kernel's registry apply step (S2) exists. Ticks are
dispatched to the plugin's tick registrations by the tests only.

Release order, at a restart and at shutdown: the registrations first, then
the plugin's admission is revoked and finished, then the plugin is destroyed
and its copy unmapped. A folder removed while the plugin is loaded changes
nothing until then: the mapped copy is private.

#### Declaring interface-7 settings and resources

`paperpaw_sdk::native::declare::Declarations` writes declaration records for
settings, groups, extras, hyphenation tables and fonts. Create it with a sink
that submits each byte slice through `registrations.submit`, add typed
declarations, then call `submit()` to send the final batch. Dropping the
builder does not submit anything.

Declaration fields borrow strings from the plugin image or from values that
live through the individual add call. The builder encodes each item immediately;
it does not keep the declarations or copy their strings. It reuses one 4 KiB
batch buffer, with bounded temporary space for a setting's default and choices.
The sink must consume each batch during its call, without retaining the slice.

A sink refusal becomes `DeclareError::Sink(status)`, preserving the host's
status. It permanently poisons the builder: every later addition and `submit()`
returns that same error, even for invalid input, without invoking the sink
again. Propagate the error from plugin construction so the host abandons its
staged declarations; never retry a partially submitted registration stream.
Conversions from older registration types exist only in test fixtures, not in
the SDK's production API.

### Packing and signing

`paperpaw-sign` (in this repository, `tools/sign`) makes the file from
the linked plugin and signs it as its author:

```sh
paperpaw-sign key author.key                      # once; keep it safe, never publish it
paperpaw-sign pack target/riscv32imc-unknown-none-elf/release/my-plugin my-plugin-x4.ppn --key author.key
paperpaw-sign plugin my-plugin-x4.ppn             # what it holds, and who signed it
```

`pack` refuses what cannot run from where the file lands: writable data
(a `static` that changes; keep state on the heap, as in `Rc`s handed to
closures), symbols it cannot find, and code that reaches an address
fixed in the firmware or ROM directly. The signature goes into the file
itself, so it travels with it; a device remembers the author's key at
the first install (from the store, the key the plugin library names for
its author) and refuses an update signed by another.

### Trying it on a device

A code plugin is a folder like a data plugin's, with its code beside its
`plugin.ini`:

```text
plugins/my-plugin/
  plugin.ini     [plugin] id, name, description, version
  code.ppn       its code, for this device and this firmware
  i18n/*.ini     translations, settings.ini, … as for data plugins
```

`plugin.ini`'s version is the plugin's: the store, the installer and
updates go by it, and the device shows it. The code's manifest must name
the same version (`paperpaw-dev package` with the folder and `--plugin`
says where it does not).

Send the folder from a computer and say yes on the device
(`python tools/usb-transfer.py install my-plugin`, or the Wi-Fi
transfer); it is loaded at once. A firmware built for development also
takes a folder over its remote control
(`tools/x4-remote.py --push my-plugin plugins/my-plugin --trust-code
my-plugin`, a folder under `plugins/`). The plugin manager lists it as one with code,
and says why one does not run (built for another PaperPaw, damaged, no
room, not installed, changed since it was installed). Over USB, `--script native` (and `--info`)
says where each plugin lies and what loading it took.

A panic's calls inside a plugin are kept with its name and the offset
into its file, e.g. `4261a402 (my-plugin+0x452)`: its function lies at
`0x42000000 + offset - 80` in the plugin's ELF. A plugin that crashes
before PaperPaw shows its first frame is switched off: after two such
starts PaperPaw starts without the plugin the crash was in (without all
plugins with code where that is not known) and says so; holding *Back*
on the X4, *Home* on the X4 Pro, while switching it
on starts without them too. The plugins it switched off are switched on
again in the plugin manager, one at a time.

### Keeping it small

On the X4 every plugin's file takes room in flash. A plugin carries its
own copy of the SDK code it uses, which is most of a small plugin: the
sample's file for the X4 is 78 KB (76 KB of code and constant data),
though the plugin itself is a screen, an action and a setting.

- Build with `opt-level = "s"` (as PaperPaw) or `"z"` (smaller, a little
  slower), `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, as the
  sample does.
- Leave out what pulls in large parts of `core`: parsing floats from
  text (`str::parse::<f32>()`, 16 KB of tables), `{:?}` of strings.
- Keep translations and other text in the folder's files rather than in
  the code, where they are needed once.

Services between plugins are found by their type's name too where the
type is the SDK's, so a plugin built apart finds those of the firmware's
plugins; one of a plugin's own type needs a contract (see [Services and
points of your own](#services-and-points-of-your-own)).

### Publishing a code plugin

The plugin library (paperpaw.org) lists a code plugin with its folder
and a build per reader (`x4`, `x4-pro`), each a signed
`.ppn` with the fingerprint it was built for, the author's key, and its
source, which the library rebuilds to compare. The sample's `build.sh`
builds, packs and signs both files, and its CI job
(`samples/hello-native/.github/workflows/build.yml`, for a plugin's own
repository) runs it on every tag, with the author's key kept as a
secret of the repository. [Publishing](#publishing) says how, and
[Which devices your plugin is for](#which-devices-your-plugin-is-for) that
a plugin may be for one reader only.

## Plugins without code

A data plugin is a folder:

```text
plugins/large-print/
  plugin.ini     [plugin] id, name, description, version, formats
  i18n/*.ini     translation catalogs (also whole new languages)
  i18n/*.ppcat   the same compiled (paperpaw-sign catalog), read as needed
  keymap.ini     key bindings
  settings.ini   new defaults for any plugin's settings ([core] font-size = 14)
  fonts/*.ttf    fonts to read with (also .otf, .ttc), by family
  fonts/*.ppfp   packs with letters other fonts lack, named in plugin.ini:
                 [extends] cyrillic.ppfp = fonts-default.literata
  fonts/…        a family's faces, named in plugin.ini: [family] name,
                 regular, bold, italic, bold-italic (packs, or .ttf/.otf
                 where scalable fonts are drawn)
  styles.css     a stylesheet for books, after a book's own (EPUB)
  presets.ini    ready-made sets of settings, e.g. themes
  marks.ini      words marked on book pages by patterns (formats 2)
  menus.ini      entries of menus, a menu of one's own (formats 3)
  actions.ini    actions made of steps (formats 3)
  icons.ini      icons of actions and settings: item = icon-name (formats 3)
  points/*.txt   lines for a point, one a line, e.g. about.lines.txt (formats 3)
  setup.ini      steps of the first-run setup (formats 3)
```

[Files on the card](data-files.md#a-data-plugins-files) lists each of them
with its format, what reads it and an example, and how a `settings.ini`
brings a home screen of its own.

`plugin.ini` may also bind fonts to roles (formats 3):

```ini
[fonts]
core.font-ui-strong = my.semibold
```

An action of `actions.ini` runs its steps in order, a step a `run`
line (commas and all), at most 16:
`<setting>=<value>` sets a setting (a value not of its kind is passed
over, and a setting that keeps the device safe, such as the lock's, is
never set), `open:<slot>` opens a slot, `notify:<key>` says the text of
a key of the plugin's translations in the reader's language,
`emit:<event>` emits an event of the plugin's own (`<plugin>.<name>`),
and an action's id runs that action (not one of the same file). A
section names the action. Without the plugin's id in front, it gets
it: `[night]` in the plugin `my` is `my.night`.

`menus.ini` adds to another plugin's menu, a section `[menu <id>]`,
places an entry after another and takes entries out; any other
section is a menu of one's own, with `title` and `entries`, which
other menus list by its id. A list is a key a line
([data-files.md](data-files.md#lists)):

```ini
# actions.ini
[night]
title = Night reading
icon = moon
run = core.night-mode=on
run = core.font-size=14
run = notify:my.good-night

# i18n/en.ini, [messages]
my.good-night = Good night, sleep well

# menus.ini
[menu core.reader-menu]
add = my.night after core.toc

[menu core.reader-more]
hide = stats.open

[tools]
title = My tools
entries = my.night
entries = core.library

# setup.ini
[size]
setting = core.font-size
order = 35
```

`formats` says which version of these files' formats the plugin is made
for (3 now: menus, actions, icons, points, setup steps and font roles
came with it, `marks.ini` with 2; 1 if it is left out); a PaperPaw
that reads an older one leaves the plugin out and says why, rather than
reading it wrongly. The files of formats 3 are read only for a plugin
that says `formats = 3`; for one made for an earlier version they are
left out, and PaperPaw says so.

A plugin's files take at most 96 KiB together besides its translations
(512 KiB of their own), those of formats 3 at
most 24 KiB of it, and its points at most 64 lines; what goes past is
left out and told. Each file takes at most 64 KiB, and `plugin.ini` at
most 4 KiB. `points/` holds at most 16 files. A default (`settings.ini`) or a preset does not
change a setting that keeps the device safe, such as the lock's.

Packs for other scripts are read a glyph at a time, so devices with
little memory can use them ([ADR 0018](adr/0018-font-packs-for-other-scripts.md)).
`tools/script-pack.py` builds Cyrillic and Greek for Literata (the data plugins
`script-cyrillic` and `script-greek`), and
Arabic and Hebrew (with where their marks sit) and Korean (the Hangul
of KS X 1001, ready as `script-korean`, and the other 8,822 syllables as
`script-korean-extra`, about 30 MiB; neither offered yet) from the fonts
in `tools/fonts`. A pack that extends a face with sizes larger than its own,
and has its letters (at least those of `aenoAEO0` the face has), draws
text asked that large, and the font size goes as far; a pack of another
script's letters adds those letters only, in whatever sizes: the X4's firmware has
Literata's regular face up to 32 px, and `tools/literata-packs.py` builds its
bold and italic faces (`literata-styles`, which the store fetches by itself
where they are missing), its larger sizes and its bold italic face as plugins.
Icon packs are such packs for the icon font: `tools/icons/pack.py` builds
one with the Tabler icons named
(`[extends] icons.ppfp = fonts-default.icons`).

A font of your own, for the X4 and the other devices that draw packs
only, is made on a computer with `paperpaw-fontgen`:

```sh
paperpaw-fontgen --family "My Font" --regular MyFont-Regular.ttf \
    --bold MyFont-Bold.ttf --italic MyFont-Italic.ttf plugins
```

It writes `plugins/font-my-font` (a `plugin.ini` with the family and a
pack for each face; only the regular one is needed, the others are drawn in
it), says how large each pack is and how many of the characters asked for
(`--chars latin,cyrillic,greek`, else Latin) the font has, and warns where
it has none. Copy the folder into the card's `plugins` folder and choose the
font in *Settings > Reading > Font*. Devices that draw font files read a
`.ttf` or `.otf` copied into the `fonts` folder (on Android
`PaperPaw/fonts` of the shared storage) instead. The fonts browser's
*Your own fonts* entry, beside *More fonts* where a font is chosen, says
this in the reader's language.

A family needs a name and a face, a family of its own a regular face;
its faces are packs, drawn on every device and read a glyph at a time,
or font files, drawn where scalable fonts are, a variable font at its
style's weight:

```ini
[family]
name = Fredoka
regular = regular.ppfp
bold = bold.ppfp
```

A plugin may also bring a face of a family that is there without it,
bound to its role: the X4's firmware has Literata's regular face alone
(no bold, italic or bold italic), which such plugins bring back (bold and
italic in `literata-styles`, which the store fetches by itself at the first
start where they are missing). Until they are there, bold and italic text
is set in the regular face:

```ini
[family]
name = Literata
bold = literata-bold.ppfp
italic = literata-italic.ppfp

[fonts]
core.font-reading-bold = literata-styles.bold
core.font-reading-italic = literata-styles.italic
```

Packs that extend a face the plugin brings name it as
`<plugin>.<style>` (`[extends] literata-bold-large.ppfp = literata-styles.bold`).

A logo of its own takes the place of PaperPaw's (on the sleep screen,
the About page and the logo widget): a picture in the folder, named in
`settings.ini`:

```ini
[core]
logo = plugins/my-brand/logo.png
```

A theme can also be a `settings.ini` with new defaults. It is then not
chosen under *Theme*; it is on while the plugin is on. Its defaults are
for the appearance settings (*Settings > Customise > Appearance*, most of them with
advanced settings): the header (`core.header.plain`, `.light`, `.dark`),
the corners (`core.corners.square`, `.small`, `.round`, `.soft`; round
and soft ones make buttons and switches pills), the highlight
(`core.focus.fill`, `.ring`, `.soft`), the lines between rows
(`core.separators.inset`, `.line`, `.none`), lists on cards
(`core.groups.cards`), the size of headers' titles, the margins, rows
and lines:

```ini
[core]
ui-focus = core.focus.ring
ui-separators = core.separators.none
ui-title = 140
ui-row = 180
```

Pebble, a theme with lists on cards for touch screens, comes with
PaperPaw, switched off in the plugin manager.

The fonts browser (`plugins/fonts-browser`) downloads fonts as such
plugins from paperpaw.org: font files for devices with scalable fonts
and the memory for them, packs for the others. The store
(`plugins/store`, *Plugin store*) lists the other plugins of the plugin
library made for the device, data plugins and, where PaperPaw loads
them and a build fits, plugins with code, and installs, updates and
removes them ([ADR 0023](adr/0023-plugin-store.md)). Both build on the
SDK's `install` module: an `Installer` downloads a `Package`'s files one
after another into `plugins/.<id>.part`, checks each by its size and
digest (SHA-1 or SHA-256), then has the kernel install it
(`Command::InstallPlugin`) in place of
every version of it before, what was chosen from it still chosen; the
job is kept on the storage, so it goes on with no screen open. Where a
plugin came from is kept once it is in place (`install::origin`): the
store updates only those it installed, and one put in place otherwise
(the Wi-Fi transfer, the USB cable, a card) is local, which the plugin
manager says. `install::local::Staging` installs a plugin sent file by
file, with the same checks.

Copy it into the `plugins` folder of a device (or of the emulator's
library) and restart, or have a plugin load it at once
(`Command::LoadPlugin` with its folder; `Command::RemovePlugin` removes
one, its fonts and presets at once, the rest after the next start). A plugin's `styles.css` comes after a book's own
stylesheets and before one's own (`styles.css` beside the books, then
the one named like a book), so the more particular wins; plugins with
code add theirs with `Registrar::add_stylesheet`.

A `presets.ini` holds presets (see above), a section each: its `kind`,
its `name` and `description` (translation keys from its `i18n`, or the
text itself), then the settings it sets by their whole id. Themes are
such presets (`kind = core.theme`, above). Pictures over the page for
the sleep screen are presets of `sleep.overlay`: the picture (or a
widget, `sleep.overlay-widget`), and where it goes and how large it is
if the preset likes (what it leaves out stays as the reader set it).

```ini
[owl]
kind = sleep.overlay
name = Owl
sleep.overlay-picture = plugins/owls/owl.png
sleep.overlay-position = sleep.position.bottom-right
sleep.overlay-size = 40
```

The picture's clear parts show the page, so a PNG with transparency
looks best. [`samples/plugins`](../samples/plugins) has large print,
three themes (airy, paw, calm ink) and a home screen (reading nook).

## Publishing

The plugin library on paperpaw.org is what the website lists and every
reader's store (*Plugin store*) installs from (ADR 0023). A plugin gets
there with the upload form at <https://paperpaw.org/submit>: sign in
with a link sent by e-mail, upload the plugin's folder as a ZIP (and a
code plugin's builds), read the checks, complete the entry, look at the
preview, and send it to the moderators. Or, without the form, tag a
release in the plugin's own repository on GitHub: the library lists it
as "Community, unreviewed" ([below](#publishing-from-github)). (The
paperpaw.org repository's catalogue holds PaperPaw's own plugins only;
it is not a way to publish.)

Before that, `paperpaw-dev check` (and `paperpaw-dev package` for a
data plugin or a `.ppn` file) finds most of what the checks would refuse,
and what they cannot see: see [Checking your plugin](#checking-your-plugin).

An update is a new version with a higher number, the same way. The
site's guide for authors, <https://paperpaw.org/authors>, has every
field, check and limit; the skill's
[`publishing.md`](../.claude/skills/paperpaw-plugin-development/publishing.md)
walks an AI assistant helping an author through every step. In brief:

### What is uploaded

The plugin's folder as a ZIP, `plugin.ini` at its top (or inside one
folder at its top), its `id` and `version` the ones published. Outside
`store/` only `.ini`, `.css`, `.ttf`, `.otf`, `.ttc`, `.ppfp`, `.txt`,
`.md` and `.png` files, at most 64 (63 for a code plugin), their paths
of letters, digits, `.`, `_` and `-`, none starting with a dot, at most
100 bytes; at most 20 MB zipped, 40 MB unpacked.

A folder `store/` in it is for the store only, and is taken out of what
readers install: `store/store.ini` gives the summary, description,
group, tags, licence, source, SDK and translations, and names the
screenshots beside it.

```ini
[store]
summary = A large clock on the home screen, with the date below it.
group = home
tags = clock, widget
licence = MIT
source = https://codeberg.org/your-name/clock
sdk = 0.5

[source]
repository = https://codeberg.org/your-name/clock
revision = v1.0.0

[screenshots]
home.png = The home screen with the clock widget, on an Xteink X4.
```

It is PaperPaw's INI: a comment is a line of its own, never after a
value.

A code plugin adds one signed `.ppn` per reader (the form's fields
`build-x4`, `build-x4-pro`) and the exact source of
those builds: `[source]` names a public repository on `github.com`,
`codeberg.org` or `gitlab.com` and the tag or commit built (with `path`
and `bin` when the crate is not at the top or the binary not named as
its package), or the source goes up as a ZIP (`source-zip`).

### Which devices your plugin is for

A plugin does not have to be for every device. Publish it for the devices
it works on and nothing else: the store, on the site and on each reader,
shows it only to those. Nobody sees a plugin that is not for their device,
and nobody is offered one that would not run.

- **A data plugin** is files, so it works anywhere PaperPaw does. Left
  alone it is offered on every device. To limit it, name the devices in
  `[store]` of `store/store.ini`, separated by commas, of `android`,
  `x4`, `x4-pro` and `x4-classic`:

  ```ini
  [store]
  devices = x4, x4-pro
  ```

  The upload form has the same as the boxes "Runs on". A device not named
  does not see it, also one of the same family: name `x4-classic` too if
  the plugin is for it. The X3 asks the store as an `x4`, so it sees what
  is for the `x4`; there is no `x3` to name.
- **A code plugin** is a build for one chip, so it is for the readers you
  give a build. Upload (or release on GitHub) one signed `.ppn` per reader
  (`x4` is the ESP32-C3 of the X4 and X3, `x4-pro` the ESP32-S3 of the X4
  Pro and the X4 Classic): its devices are those, whatever `devices` says,
  and a build for only the X4 is a plugin for the X4 only. A reader
  needs the build made for its own fingerprint; one that has none for it
  (an older release, a chip you did not build for) does not list the
  plugin, and the site shows it as waiting for a build. Android takes no
  plugin with code from the store, so a code plugin never shows there.
- **Where it shows.** The plugin's page and the library's lists on
  paperpaw.org say "X4 and X4 Pro only" for a plugin that is not for all,
  and the library can be filtered by device. The reader's store (*More
  plugins*) asks for its own device and lists only what fits it. A
  catalogue entry (`catalogue/<id>.toml`) names `devices` the same way,
  and every build it lists must be for one of them.
- **Later.** A new version may add a device: add its build (or name it in
  `devices`) and publish the version.

### The author's key

Code plugins are signed by their author with an Ed25519 key of their
own, made once:

```sh
paperpaw-sign key ~/.paperpaw/author.key     # prints the public key, 64 hexadecimal digits
```

The file is the secret key: keep it outside every repository, keep a
copy apart, never commit or share it. In CI it is a secret of the
repository (`PAPERPAW_PLUGIN_KEY` in the sample's job); the site's page
<https://paperpaw.org/authors/sign> also signs in the browser, the key
never leaving the computer. The store reads the public key from the
builds: all of them are signed by one key, and every update by the same
one. Readers remember it at the first install and refuse an update, or
a new install after a removal, signed by another: a new key means a new
plugin for the readers who have it. A lost key is changed only in the
open, through the site's contact address.

### Builds that match their source

The site keeps each code plugin's source, shows it file by file, and
rebuilds each build in a container with the toolchain its fingerprint
names, with the sample's aliases (`cargo x4`, `cargo +esp x4pro`,
`--locked`). It compares the result byte for byte with the build
without its signature, and the plugin's page says *Verified: matches its
source*, *Not rebuilt* or *Rebuilt, but does not match its source*.
Paths end up in a build (in its panic messages), so the sample's
`build.sh` and the rebuild write them as the same everywhere, with
`--remap-path-prefix` added to the flags of `.cargo/config.toml`
(through `cargo --config`): the source's top as `/plugin` and Cargo's
home as `/cargo`. The core libraries come built, the same for everyone:
rustup's for the X4, the release's for the X4 Pro, which the rebuild
installs as authors do (see [The crate](#the-crate)). So:

- build with `build.sh`, or with the sample's CI job that runs it
  (`samples/hello-native/.github/workflows/build.yml`). Build from a
  pushed tag, in any folder, on Linux or macOS. On Windows, paths inside
  the source keep their `\`, and such a build matches only builds on
  Windows;
- commit `Cargo.lock`;
- keep the release's compilers pinned (`rust-toolchain.toml`, the job's
  `RUST_X4`, `RUST_X4_PRO`, `ESP_GCC`), its SDK
  (`paperpaw-sdk = "0.6.0-beta.3"`) and, for the X4 Pro, its core libraries
  installed, and build again for every release until 1.0 (see
  [Which build fits](#which-build-fits));
- never set `RUSTFLAGS`, which would replace the flags that link the
  plugin, and never publish a build made without the remapped paths;
- put nothing of the moment or the computer into the code (a build
  script writing the time, a user name, a random value);
- name dependencies by version or a fixed commit, never a branch, and
  never by a `path` outside the repository (its folder is not remapped,
  and it changes the hashes Cargo gives every crate).

The sample's `build.sh`, which the job runs, builds both readers, packs
and signs the files, and zips the folder, with and without `store/`; the
job attaches them to the tag's release with their SHA-256. Building twice, in two folders, and
comparing the files with `cmp` shows that the build holds.

### Publishing from GitHub

A plugin can also reach the library from its own repository, without the
form. Give a public repository the topic `paperpaw-plugin` and publish a
GitHub release: paperpaw.org looks through such repositories every hour,
takes each one's newest release, checks it as it checks an upload, keeps
its files and lists the plugin as "Community, unreviewed", with the
GitHub owner as author, a link to the repository and the key's
fingerprint. Readers never download from GitHub; the site serves the kept
files.

The release's tag is `v` and the version of `plugin.ini`. Its assets are
`<id>-<version>.zip`, the plugin's folder as the form takes it (with
`store/`), and for a code plugin one signed build per reader,
`<id>-<version>-<device>.ppn` with the device `x4`, `x4-pro` or
`x4-classic` (the X3 runs the X4's). A release must not be a draft or a
pre-release. The source of a code plugin is `[source]` in
`store/store.ini`, or else the repository at the release's tag.

Start such a repository with `paperpaw-dev new --github <id>` (`--code`
for a plugin with code): it lays out the plugin's folder, `store.ini`
with a valid group, the release workflow and a README. Before every tag,
`paperpaw-dev check-publish` (or the smaller `paperpaw-sign
check-publish .`) checks the repository by the rules the site checks by,
`tools/publish/store-rules.json`, the same file the site serves at
`/api/v1/store-rules.json`, and says what to change.

[`paperpaw-release.yml`](../tools/dev/templates/README.md#publish-by-tagging)
in `tools/dev/templates/.github/workflows` does it all on pushing the
tag: it checks the tag against `plugin.ini`, runs `check-publish`, zips
the folder, builds and signs a code plugin with the key in the
repository's secret `PAPERPAW_PLUGIN_KEY`, checks the files it made, and
creates the release only where every check passed.

The site refuses most often: a group that is none of the twelve, more
than five tags, a screenshot without its line in `[screenshots]`, a
summary longer than one sentence, files in the plugin's folder that are
no plugin's (a README or LICENSE where `PLUGIN_DIR` is the repository's
top), a tag that is not `v` + the version, a build not signed or signed
by another key, and for code a missing `Cargo.lock` or the SDK named by
path. `check-publish` names each with its fix.

Unreviewed means the moderators did not read it; the checks are the same.
An id belongs to whoever it first appeared with, reviewed or from
GitHub: its key, as in [The author's key](#the-authors-key), or for a
data plugin its account or GitHub owner. A release with another's id or
key is not offered; the ids and names of PaperPaw's own plugins are
reserved. Readers refuse an update signed by another key. The admins can
block a repository, an owner or a key, which takes its plugins down at
once. To have it reviewed later, name the repository in the store's form
(*From GitHub*): from then on each new release waits for the moderators
as an update. The skill's
[`publishing.md`](../.claude/skills/paperpaw-plugin-development/publishing.md)
has the steps.

[`Plugin`]: ../crates/paperpaw-sdk/src/plugin.rs
[`Registrar`]: ../crates/paperpaw-sdk/src/contribution.rs
[`Env`]: ../crates/paperpaw-sdk/src/env.rs
[`Command`]: ../crates/paperpaw-sdk/src/command.rs
[`Screen`]: ../crates/paperpaw-sdk/src/screen.rs
[`Widget`]: ../crates/paperpaw-sdk/src/widgets.rs


### Document-scoped storage

`native::storage::Root::Install` (root 2) reads the plugin's own install folder,
read only, in any callback. During a permitted format callback, `Book` selects
root 3, `UserCss` root 4 and `DocumentCache` root 6. An empty path names the book
file on root 3 or the CSS file on root 4. Folder and siblings formats may read
relative paths inside root 3. Root 3 is read only; root 4 is read only and
available during `core.format` ops 2 and 3 only. The host enforces the callback scope,
cache quota and reserved `.complete`/`.host/` cache paths. The host writes
`.complete`; a format never writes it. Use the current callback's `Host`
for storage calls and never retain it across callbacks.

### Extension stages and call services

Declare an extension point with `native::declare::StageDecl` and subscribe with
`SubscribeDecl`, passing it to `Declarations::stage` or `subscribe`. Grammar and payload ranges are
checked locally. Stage ownership, self-subscription, duplicate stage/token pairs
and the eight-per-plugin subscription limit are checked by the host at commit.
A subscription's `order` controls its position and its `signature` contributes to
layout invalidation when the stage changes layout.

A subscriber receives `native::events::Event::StageCall { stage, schema, payload, .. }`
at its declared token. Check the stage and schema, decode `payload` with the
stage owner's schema crate, and write the encoded reply using the existing
`native::reply::Reply`. Return `Error::Unsupported` for a schema the subscriber
does not understand. Plugin-defined stage payloads remain opaque.

For `core.widget` major 1, use `native::ui::stages::WidgetCall::decode(payload)`
to read measure, place and target requests. Reply with `reply_size`,
`reply_place` or `reply_targets` into caller-owned scratch, then pass the encoded
bytes to `Reply`. Targets stay in reading order; more than 32 returns
`Error::NoSpace`. For `core.status` major 1, `StatusCall::decode(payload)` exposes
borrowed requested item ids and context params. Build `StatusAnswer::text` or
`StatusAnswer::drawn` values and encode with `StatusCall::reply`. Replies accept
at most eight requested items and refuse duplicates. These adapters use the
generated directional codecs and allocate no heap storage.

`native::stages::Stage::new(host, "reader.insert")` binds a full stage id.
The host refuses another plugin running it with `Error::Denied`.
`native::stages::StageRun::next` lends one `StageReply` at a time from the owner's
reply buffer. Inspect `status()` before using `payload()`; cancellation, busy and
invalid subscriber replies still advance the cursor. Only `style_granted()`
permits an override of a user-protected style value. Size the buffer with
`Stage::out_len(max_reply)`. End-of-stage and host refusals finish the cursor.

`native::services::Service::open(host, key, major, minor)` opens a generated call
service schema. Keep the handle across callbacks and pass each callback's current
host to `call`. Use `max_reply(op)` to size the output buffer. The SDK checks the
operation version and buffer limits before calling the host. Explicitly consume
the handle with `release(host)` when done; teardown also releases it. There is
no automatic Drop release because the handle retains no callback host. Per-family
typed wrappers define the input and output records.
### Format provider callbacks

A format's declared token receives `core.format` major 1 stage calls. Decode
its payload with `native::format::FormatCall::decode` and dispatch using
`FormatOp`. These provider operation ids differ from the document service's
ids. The call borrows its text and sniff bytes; `document()` and `conversion()`
identify the state the provider owns for this document. Close releases it.

Use `FormatState::preparing` to report incremental work and `ready` or `failed`
to finish preparation. `reply_state` keeps the call's document and conversion
identity. Scalar reply helpers validate the request operation before encoding
into caller-owned output. Pass the returned bytes through `native::reply::Reply`.

Blocks (op 8) are written with `FormatCall::reply_block(&BlockReply::new(&block)
.next(offset), out)`, adding `.image_index(n)` for a picture block and `.last_in_section()`
for the section's last block. The reply is one fragment of the block's record list,
from the record the call's `cursor` names; when the block does not fit, the fragment
ends with `more`, the next record index and the block's generation, and the host asks
again with them (a changed block is answered `State`). `out` of 3,840 bytes always
holds a fragment.

Lists (sections, table of contents, links, fonts) are written with
`FormatCall::rows(out)` and its typed row methods. A row that does not fit is
`NoSpace`: end the page with `PageEnd::more(total, next, generation)`; the host asks
again with `next` and that generation. The last page is `PageEnd::last(total, generation)`. `total` is the whole list's row
count and `generation` the format's own for this document, never 0, the same on every
page; a call with a stale generation is answered `State`.
Pages always keep room for their closing record. A link whose `href` is over 1,024
bytes is skipped (`Ok(false)`). `reply_image` answers cover and image requests with
the cache path, the picture's own index and size.

`reply_metadata` borrows the existing `document::Metadata`; title and series
previews advertise UTF-8 cuts with flags. `reply_metadata_text` returns the
requested value in UTF-8 parts, preserving its total byte length. It allocates
no strings and refuses offsets inside a character. `reply_position` preserves
the provider's source section and offset without reader insert ordinals.

During a permitted format callback, `native::storage::Root::DocumentCache`
selects root 6, `Book` root 3 and `UserCss` root 4 (root 2 is the plugin's install folder). An empty path names the book
file on root 3 or the CSS file on root 4. Folder and siblings formats read
relative paths within root 3; for siblings, the call's `text()` names the
opened file unless the operation uses text for an XPointer or href. Root 3 is
read only. Root 4 is read only and available during open/prepare only. The host
enforces the callback's scope, cache quota and reserved `.complete`/`.host/`
paths. Never retain a callback's `Host`; `.complete` belongs to the host.

### Secrets and device randomness

Open `native::secrets::Secrets` once with the metadata-declared `core.secrets`
service, use the current callback's `Host` for each operation and call `release`
when finished. The handle retains no callback binding. `get` accepts an own
unprefixed name; it cannot request another plugin's value. Names and labels
preserve SSID tab, LF and CR characters while refusing other controls.

`SecretWrite::new(SecretName::new(name)?, value)` saves a complete value, including
an empty value. `.label(text)` replaces a label; an empty label preserves an
existing one. `.changeable()` permits editing by the user. `keep` clears the
encoded input buffer before returning, including after a refusal; its borrowed
source value remains the caller's responsibility. `remove` accepts a `SecretName`.

The settings editor declares `uses.secrets`. It lists `SecretList::all()`;
ordinary plugins use `SecretList::own()`. A list page contains borrowed names,
labels and changeable flags, never values. Continue from `page.end().next()`
using the same query's `.from(cursor)`. Concurrent edits may change row order
or totals between pages. `entry.target()` names an owner for removal, and
`entry.replacement(value)` preserves its changeable flag for replacement.
Other owners' values cannot be read, even by the editor.

`random(host, len, input, out)` draws exactly 1-256 bytes. Inspect `bytes()` and
`strong()` on the returned borrowed view. A host without a source refuses the
call; the facade supplies no substitute source. Lock may use a weak draw as a
salt under its contract; callers needing a strong key check the flag. Secret
values and random bytes are redacted in Debug output.

Input buffers of 1,280 bytes cover all calls. Output needs 4,096 bytes for list
or 1,280 bytes for get, keep, remove and random. Views cannot outlive or reuse
their output buffer while borrowed; a caller clears its value output after use.

### Book key and reading position

`Library::book_key(host, book, input, out)` returns the book's key (the folder name for
the plugin's own per-book data, changed by a move) and its KOReader fingerprint and
size, which together identify the book on other devices. Keys come only from the host.
`Library::position` reads the stored position; `Library::set_position` writes a
`PositionWrite`. A local write always wins. A `.sync()` write wins only over an older
sync position; otherwise it answers `State` and stores nothing, so the plugin can ask
the user and write again with `.confirmed()`. `.confirmed()` without `.sync()` is
refused as `Invalid` and stores nothing. `out` of 4,096 bytes always suffices. The input
buffer is 1,024 bytes for `book_key` and `position` and 2,048 bytes for `set_position`.

### Pictures

The host decodes and draws pictures; a plugin never holds one in memory.
`CommandSender::image_prepare(&PrepareImage)` asks for a JPEG, PNG (not interlaced) or GIF
of the plugin's data folder, with `uses.storage` the library, or (`PrepareRoot::Bundle`, no
feature) the plugin's own install folder, read only: the pictures it ships; a plugin built into
the image has none there, and the host answers `ImageStatus::NotFound`
(`PrepareImage::new(PrepareRoot::Data, path, cookie)`, `.side(n)` for the longest side, 0
being the screen's and a side above 1,600 being clamped, `.colour()`). Subscribe to
`EventKind::ImageReady` first: the end is one `Event::ImageReady` carrying the cookie, an
`ImageStatus` (`Ready`, `Unsupported`, `Damaged`, `TooLarge`, `CardFailure`, `NoMemory`,
`NotFound`: no such file at that root and path; an unknown value reads as `Damaged`), and for
`Ready` the `ImageId` and the size made. At
most 4 requests wait and 8 pictures are kept per plugin; give each back with
`image_release(id)`. `ImageInfo::read(host, id)` gives the size and whether it is colour
(`image.info`; `not_decoded()` says the format has not decoded it yet), and
`Canvas::draw_image(id, rect, ImageStyle)` draws it scaled into the rectangle, centred,
clipped by the canvas, only inside a draw callback (`State` elsewhere).
`ImageStyle::new().fill()` crops to fill, `.stretch()` scales to the rectangle and drops the
shape, `.dots()` draws black and white dots also on a panel with grays and `.grays()` in the
panel's grays, `.small()` keeps a small picture out of the ghosting refresh,
`.transparency(0..=255)` blends (255 draws nothing). `PrepareImage::turn()` applies a JPEG's EXIF
orientation (the size in `ImageReady` is the size after the turn), `.tone(&[u8; 256])` maps every
gray through the plugin's own table while the picture is decoded, and `ImageReady::luma()` is the
mean brightness of an 8 by 8 grid over the picture. `NotFound` is an
ended id: prepare it again. The host reads the file in bands, so a card failure can leave
the bands drawn so far.

`Library::cover(host, book, side, input, out)` gives a book's cover as an `ImageId` with its
size (`Cover::image()`, `Cover::info()`), made by the book's format on first use and kept in the
host's cover cache. `side` 0 is the list's size. `NotFound` means the book has no cover (show
text); `Busy` means it is being made, so ask again when `library_changed` bit 6 says it is
ready. Draw it with `Canvas::draw_image` and release it with `image_release` when it scrolls out
of the list. `input` is 1,024 bytes and `out` 4,096.

### Opening a book as a client

`native::document::Documents::open(host)` opens `core.document`. `open_book(host, &OpenBook, input, out)`
opens one book and returns a `DocState`; `OpenBook::new(path)` reads the card's library
with the host's format and picture sizes, and its builders set the root, the format,
flags, picture sides (at most 1,600), a user CSS file and up to 12 `(name, value)`
options. More options than 12, or one over its caps, answers `NoSpace`; the handed root
for the CSS file is `Invalid`. Nothing bad reaches the host. `state` reads the
preparation state (`Preparation`), `close` ends the document, `xpointer` turns a
`DocPosition` into a KOReader XPointer and `locate` turns an XPointer (at most 1,024
bytes) back. An answer for another document is `Invalid`. Reading blocks and the
table of contents are not in this client yet. The host answers `Busy` while a
document is preparing.

`Documents::metadata(host, document, input, out)` reads what the format says about the book:
`title()`, `language()` (a BCP 47 tag, empty for none), `series()`, `series_index()`,
`right_to_left()`, `writing_mode()`, `sections()`, the counts `authors()` and `subjects()` and
`description_bytes()`, with `title_cut()` and `series_cut()` where a text was cut for the
reply. `Documents::text(host, document, DocTextQuery::description(), input, out)` reads any
text whole in parts of at most 1,024 bytes (`title()`, `author(index)`, `subject(index)`,
`description()`, `series()`; `.from(offset)` and `DocTextPart::next_offset()` continue; an
index past the count is `NotFound`). `input` of 64 and `out` of 4,096 bytes suffice.

`Documents::readers(host, scope, cursor, input, out)` lists the reading apps for "Open with" and
the default reader, in pages of at most 64 rows: `scope` keeps the readers that show the format
with that id (empty for all), `ReaderPage::rows()` gives each `Reader` (`id()`, `screen()`,
`title()` and `formats()`, an empty list meaning every format: `reads_every_format()`), and
`ReaderPage::end().next()` is the cursor of the next page, `None` after the last. A cursor whose
list changed answers `State`: start again from `ReaderCursor::start()`. No permission is needed.
A reader opens a book through `command.open` with its `screen` and the slot `core.reader`.
`input` is 256 bytes and `out` 4,096.

`Documents::format_for(host, root, path, sniff, input, out)` tells which format reads a file
(`Root::Data`, or `Root::Library` for the user's files, which needs `uses.storage`): by its extension,
and when none matches and `sniff` is set, by the file's first 512 bytes. The answer is a `Format`
(`id()`, `title()`, `extensions()`, `priority()`); `NotFound` means no format reads it, so a file
manager offers Open only where there is one, and `Documents::readers(host, format.id(), ..)` lists
the readers for Open with. `Documents::formats(host, cursor, input, out)` lists every format in
pages (`FormatPage`, `FormatCursor`). `input` is 1,024 bytes for `format_for` and 256 for
`formats`; `out` 4,096 for both.

### Annotation anchors and per-book folders

`Library::anchors(host, book, cursor, input, out)` pages through the anchors of a book's
annotations: for each annotation that has them, `Anchor::id()` (the annotation's id),
`conversion()` (the conversion its offsets belong to, `Some(0)` differing from `None`) and
`xpointers()` (the start and end KOReader XPointers, at most 1,024 bytes each; one that is
longer reads as absent). The page's end carries the book's annotation generation like
`Library::annotations`; continue with its cursor and start again after `State`. A reader whose
conversion differs relocates an annotation through its XPointers (`Documents::xpointer` and
`locate`) and writes it back. `input` is 1,024 bytes and `out` 4,096.

`Declarations::book_data(name, BookDataKind::UserData | Cache)` declares a folder the plugin
keeps per book at `books/<name>/<book key>/` in its data (the key from `Library::book_key`): user
data moves and goes with the book, a cache is made again from the book and also goes when the
book is no longer found. The host moves and removes the folders; the plugin only writes inside
them. The name is one folder name (lowercase letters, digits and `-`, at most 96 bytes; no dot, no
separator), and the host allows at most 4 per plugin.

### Staging annotation edits

A plugin that saves a whole merged set of annotations (KOReader) stages them in a transaction:
read the book's annotations and keep the annotation generation of the last page, then
`Library::edit_begin(host, book, generation, input, out)` gives a `Transaction`. Stage edits with
`edit_add(host, &mut tx, &[AnnotationEdit], input 4,096, out)`, at most 3 per call (more is
`NoSpace`): `AnnotationEdit::add`, `upsert` (by `client_key`, so a repeated import adds nothing
twice), `replace(id, ..)` and `remove(id)`, with `.percent()`, `.conversion()`, `.times()`,
`.client_key()`, `.text()`, `.note()` (at most 3 KiB) and `.xpointers(start, end)` (1 to 1,024
bytes each). The SDK sends a text or note over 512 bytes and every XPointer in parts, cut on
character boundaries; limits are checked before anything is sent. `edit_commit` applies all edits
or none: `State` means the book's annotations changed since `edit_begin` (read again, merge, begin
again). A plugin advances its own sync state only after a commit answered `Ok`. After an error from
the host while staging, call `edit_abort`: the transaction refuses further staging and commit
(`State`). One transaction is open per plugin and two on the device (`Full`); it ends on commit,
abort, ten idle minutes or when the plugin stops. `edit_receipt(host, &committed, cursor, input,
out)` then pages the annotation ids the edits made, `Receipt::{edit, id, action}`, until the
plugin's next `edit_begin`. Every receipt page carries the annotation generation the commit
answered; a page of another generation is `Invalid`.

### Deleting and importing books

`Library::can_delete(host, paths, input, out)` asks, for 1 to 7 card paths, which books
can go and what would go with them: each `DeleteRow` has `status()` (`Ok`, `Invalid` for
not a book, `NotFound` for gone already, `Io`) and `has_annotations()`,
`has_position()` and `has_book_data()`, so a view can ask the user first.
`Library::delete(host, paths, with_notes, input, out)` removes the books and everything
kept for them; a book with annotations answers `State` in its row unless `with_notes`,
which a plugin passes only for books it downloaded itself or after asking. All three need `uses.library`; `delete`
also needs `uses.storage`, `import` does not. The host refuses
paths whose first segment is `plugins` or `.paperpaw`. Both answer one row per path, with
that path, in the order asked; a reply that does not is `Invalid`. `input` is
4,096 bytes; `out` is 4,096 bytes and also holds the joined paths until the call.

`Library::import(host, &Import, input, out)` puts a downloaded file from the plugin's
root 0 into the library and returns the book's library path, borrowed from `out`.
`Import::new(from, name)` names the source and the file name; `.folder("a/b")` puts it
below the library folder; `.existing(Existing::Replace)` replaces a book this plugin
imported earlier (it keeps its position and annotations), `Existing::FreeName` takes
`name (2).ext`, and the default `Refuse` answers `State`. Names are checked before the
host is called: no empty, `.` or `..` segment, no path separator in the name, no control
character or any of `"*:<>?|`, no trailing dot or space, no reserved device name
(`CON`, `NUL`, `COM1`…), at most 255 UTF-16 units per segment, no name starting with `.`
or `~`; all are `Invalid`. `input` is 2,048 bytes and `out` 4,096. Downloaders need no
storage permission for it.
### Opening a book as a client

`native::document::Documents::open(host)` opens `core.document`. `open_book(host, &OpenBook, input, out)`
opens one book and returns a `DocState`; `OpenBook::new(path)` reads the card's library
with the host's format and picture sizes, and its builders set the root, the format,
flags, picture sides (at most 1,600), a user CSS file and up to 12 `(name, value)`
options. More options than 12, or one over its caps, answers `NoSpace`; the handed root
for the CSS file is `Invalid`. Nothing bad reaches the host. `state` reads the
preparation state (`Preparation`), `close` ends the document, `xpointer` turns a
`DocPosition` into a KOReader XPointer and `locate` turns an XPointer (at most 1,024
bytes) back. An answer for another document is `Invalid`. Reading blocks and the
table of contents are not in this client yet. The host answers `Busy` while a
document is preparing.

### Provided metadata

`Library::provide(host, book, &Provided, input, out)` (needs `uses.library`) sets what another
source knows and the book's file does not name, as calibre does for a book it sent:
`Provided::new().title(..)?.author(..)?.series(..)?.number(..)?.tags("a\nb")?` (title and author
at most 512 bytes, series 256, number 32, up to 32 tags of 1 to 128 bytes one per line, 2,048 in
all). Only the fields set are provided, the rest are left alone: a value is never empty or blank
(only Unicode whitespace; nothing clears a field: leave it out) and has no control
character (`char::is_control`: C0, DEL and C1, so a tab or line break either, and U+2028 and U+2029; a tag list separates its tags by line breaks only), and tags replace the provider's whole tag list. Provided
values fill what the book's file does not name: where the file names a field, its own value wins
on (re)index. The last provider's values replace earlier ones, and there is one tag list per
book. They are kept with the book's data and follow it when the book moves. `input` is 4,096 bytes and `out` 64.
`Documents::format_for(host, root, path, sniff, input, out)` tells which format reads a file
(`Root::Data`, or `Root::Library` for the user's files, which needs `uses.storage`): by its extension,
and when none matches and `sniff` is set, by the file's first 512 bytes. The answer is a `Format`
(`id()`, `title()`, `extensions()`, `priority()`); `NotFound` means no format reads it, so a file
manager offers Open only where there is one, and `Documents::readers(host, format.id(), ..)` lists
the readers for Open with. `Documents::formats(host, cursor, input, out)` lists every format in
pages (`FormatPage`, `FormatCursor`). `input` is 1,024 bytes for `format_for` and 256 for
`formats`; `out` 4,096 for both.

### Book fields and badges

Declare an owned field with `native::declare::BookFieldDecl::new(name, title)`
and `Declarations::book_field`; `.in_catalog()` also keeps short values in the
catalog. `Declarations::book_badge(name, token)` registers the owner callback.
The host supplies each name's plugin prefix. Badge stage payload codecs follow
Kernel's `core.book-badge` schema.

Read all owners' fields with `native::library::Library::fields`, passing a
`BookRef` and `FieldCursor::start()` for the first page. `FieldPage::rows()`
lends each complete id, translated title and value. Continue with
`page.end().next()` until it is `None`; no value is silently shortened.
The cursor carries this book's field generation. It is distinct from the
catalog generation carried by a catalog-based `BookRef`. `Error::State` on
continuation means restart from the first page. At most 64 fields belong to
one book, in owning-plugin/name order; each page fits the service reply cap.

### Core event subscriptions

`native::declare::EventKind` includes setting changes, book opened/closed/finished,
book moves/removals, plugin changes, Wi-Fi, power mode, status and charge changes.
Subscribe with `Declarations::subscribe_event(kind, token)`. A host returns
`Error::Unsupported` until the relevant producer is implemented; these declarations
never start a stopped plugin.

The matching `native::events::Event` variants borrow callback bytes. Book session
edges use `Event::Book { kind, reason, reading, .. }`; page turns and book edges share
`ReadingView`, whose accessors expose the complete path, reading position, titles
and optional page counters. Reason 1 denotes a restored open or a sleep/shutdown
close; reserved reasons become 0. Book-finished reason is always 0.

`Event::SettingChanged` provides an optional typed previous value. A missing or
unreadable previous value is `None`; malformed nonempty settings values are refused.
An empty key is an overflow signal: reread settings retained by the plugin. Read
new values with `native::settings::read_setting`.

Wi-Fi state 4 becomes `WifiLinkState::AccessPoint`; reserved states become `On`
without a connection. `PowerChange` describes the power mode and energy level,
including automatic selection and work stretch. It borrows the mode id and
normalizes reserved energy levels to Normal. `ChargeFlags` separately describes
charging, external power and low battery; unknown flag bits are ignored.

Unknown appended optional fields are accepted, while malformed framing, strings,
reading counters and setting values are refused. `event_dropped` is a host-shell
reporting path, logged by the host; it is not an event delivered to plugin code.

### Flat callbacks in interface 7 draft 5

The flat `native::Plugin` facade borrows all callback input from the host.
`event` receives a decoded `native::events::Event` and a `native::reply::Reply` writer: append the family's encoded
reply with `write`, and return `Ok(())`. The glue returns only the written length.
A full writer returns `Error::NoSpace` without appending a partial write.
Events without replies receive an empty writer.

`draw` receives a decoded `native::callbacks::DrawFrame` with its target rectangle,
clip rows and borrowed payload. `input` receives a typed `Input` and returns
`Handled::Yes` or `Handled::No`. Neither input nor reply storage can escape the
callback. Decoders reject unknown exact tags and invalid input enum values.

Declare screens and actions with `native::declare::ScreenDecl` and `ActionDecl`.
Choose repeated-open behavior with `ScreenDecl::instances(native::declare::Instances::Single)`;
`PerParams` is the default and `Many` creates an instance for each open.
Use `Question::confirm(...)?` with `.confirm_labels("Keep\nDiscard")?` for custom
button words. Empty labels retain the dialog defaults. `.only_yes()?` requires an
empty no label; `.focus_yes()?` selects initial yes focus. Dialog plugins read
`QuestionInfo::{yes_label, no_label, only_yes, focus_yes}`.
For menu discovery, `RegistryItem::user_defined()` reads the host's explicit user
definition flag, including empty definitions and definitions equal to defaults.
Decode their shown/run events with `native::events::Event::decode`. Format catalog strings
with `native::formatting::format_message(host, key, args, &mut buffers)`.
Create `FormatBuffers::new(scratch, out)` to name the temporary argument encoding
storage and translated output separately; neither buffer is allocated by the SDK.
`digits` maps ASCII digits. Both functions return the full required byte length,
so retry when the result exceeds the output capacity.

### Native Bluetooth

Declare `uses.bluetooth` in the metadata for the commands; reads of the radio and the bonded devices
(`Device::bluetooth`, `Device::bonded`) need none. `CommandSender::bt_scan(ms)`, `bt_stop_scan`,
`bt_pair(device)`, `bt_connect`, `bt_disconnect`, `bt_forget`, `bt_give(device, plugin)`,
`bt_take_back` and `bt_keys(device, on)` queue one command each and do not wait; the answers come as
`Event::Bluetooth` to the plugin bound to the host slot `core.bluetooth` (offer a screen for the slot
with `SlotOfferDecl`; no event subscription exists for it). Device ids are those of the reads. The
event carries `kind` (`Changed`, `Found`, `ScanEnded`, `Connected`, `Disconnected`, `Paired`, `Done`,
`Failed`), the device and its kind, the signal in dBm (`None` where not known) and a `reason` (`None`
where none is said); unknown values read as `Other`. Its name borrows the event.

### Native library access

Open `native::library::Library` during a callback and release the service handle
explicitly when finished. Declare `core.library` and request `uses.library` in
metadata. Build borrowed filters with `Query::new().show(Show::Unread)` and
`.words(search)?`. `Library::query` returns a page borrowing the output buffer;
read `page.end().next()` to continue the same query with `.cursor(next)`.
A stale generation returns `State`; start the query again instead of skipping rows.
`Entry::badges()` exposes the host's badge texts without a second lookup.

Identify a book with `BookRef::path(path)?` or a catalog key and its page generation.
`set_finished(host, book, true, input, out)` changes only the finished status and
`set_favourite(host, book, true, input, out)` only the favourite status; both are idempotent.
`Library::book_text(host, book, BookTextQuery::title(), input, out)` reads a whole title,
`author()`, `series()`, the tag at `tag(index)` or the plugin field at `field(index)` (as
`id=value`), the lists' lengths being `Book::tags()` and `Book::fields()`, in parts of at most
1,024 bytes: continue with `BookTextPart::next_offset()` (`.from(offset)`) until it is `None`
(`input` of 1,024 and `out` of 4,096 bytes suffice; an index past the count is `NotFound`).
The catalog rows and `Library::book` cut long values; this does not.
`set_field` takes the full `plugin.name` field id owned by the caller; an empty
value removes it.
The SDK leaves field enumeration out until the host contract supports lossless
paging for all fields. Reply and input buffers belong to the caller; queries
allocate no SDK heap storage.

### Native device inventory

Open `native::device::Device` from a callback after declaring `core.device` in
metadata. Its inventory reads include `device`, `firmware`, `native`, `memory`,
`values` and `builtins`; radio, power and request-status reads use the same handle.
Call `release(host)` explicitly when the handle is no longer needed.
Replies borrow the caller's output buffer, and the SDK allocates no reply storage.

Use `DeviceCursor::start()` for a paged list and continue only with
`page.end().next_cursor()`. A `State` error means restart from the first page.
Hardware values and physical keys are complete unpaged lists; their terminal
metadata is checked before rows are exposed. Optional memory readings return
`None` when the host did not measure them. `FileCheck` is host evidence, not a
local trust decision; installation still goes through the host confirmation flow.

### Native host wrappers for the Battery pilot

Menu declarations use `native::declare::{MenuEntryDecl, EntryPlacement, MenuSectionDecl}`.
Construct a `MenuEntryDecl::new("core.tools", "battery.open")`, set `order = 37`,
then add it through `Declarations::menu_entry` and assign the icon through
`icon("battery.open", "battery")`. Entry and section order are typed `i16` values.
`EntryPlacement` carries an anchor only for after, before and section placement.
Entry items, icon items and device-need items are full ids; foreign items are allowed.
`menu_need(item, MenuNeed)` uses the existing SDK need enum. `menu_section` takes
a `MenuSectionDecl`; `hide_menu_entry(menu, item)` uses full ids, while
`describe_item(item, key)` names an owned item relatively. The builder checks
syntax and generated field limits; the host resolves ids and checks duplicates.
Hosts without menu wiring return `Unsupported` rather than dropping declarations.
These methods also work in build scripts that emit static batches. Card catalogs
remain at `plugins/<id>/i18n/<language>.ini`, with language fallback ending in English.

`native::declare::Declarations::subscribe_event(EventKind::Started, token)`
registers a typed event subscription; `register_draw(token)` registers a draw
callback. `EventKind` lives in `native::declare` and covers tick, lifecycle,
battery, page, screen and action events. These methods use the same ordered,
bounded batches and failure latching as settings, screen and action declarations.
They also work in build scripts that emit static registration batches; a plugin
never needs to write callback record bytes or import private generated codecs.

`native::snapshot::capture_snapshot(host)` returns an owned snapshot with battery,
display and time getters. `native::settings::read_setting(host, key, scratch)`
returns a borrowed `SettingValue`; `Absent`, false, zero and empty text remain
distinct. Grant reads use only `settings::own_grant(host, GrantKind)` and the host
checks the caller. Generic grant reads and typed grant writes are refused.

`native::formatting::copy_message(host, key, out)` returns complete borrowed UTF-8
or `NoSpace`. `MAX_MESSAGE_BYTES` sizes one reusable output buffer. Number labels
can look up `core.decimal` (falling back to "." when missing), format fixed-point
ASCII text, then use the digit conversion slot.

A draw callback obtains its canvas with `frame.draw_canvas()`. A frame decoded outside
entry glue has no canvas access and returns `State`. The non-copyable frame is borrowed mutably for the canvas lifetime;
dropping the canvas allows another borrow. Canvas primitives preserve
the host's frame clipping and colour conversion. `Font::acquire_role` borrows the
callback host, `measure_text` returns advance, and `render_text` takes that canvas
and a `TextOrigin`. Consuming `release` and `Drop` each release a font once. A
font cannot be drawn through a different host binding.

REG declarations use `SetupStepDecl`, `SlotDecl`, `SlotOfferDecl`, `KeyBindingDecl`,
`PresetDecl` and `PresetValueDecl` through `native::declare::Declarations`.
`setting_default` and `setting_choice` extend another plugin's settings; the host
checks authority and resolves extensions in plugin load order. Setup conditions
are declared values and device requirements, with no callback during a query.

Registry commands preserve the difference between a default and an empty user
configuration: `set_menu(menu, None)` restores declared rows, while
`set_menu(menu, Some(&[]))` clears the custom menu. Menu and plugin request lists
are checked against the generated limits and never truncated. A preview set is
submitted as one batch with caller-owned value scratch; oversized previews fail
before submission. Use a preset preview for larger sets. The host checks preview
ownership and obtains any required user consent for mass resets.

`native::commands::CommandSender::new(host, scratch)` stages typed setting changes
with the generated encoder. Each accepted call is one atomic batch, applied by
the host after the callback. Dropping the sender submits nothing.

`Font::read_line_metrics` returns the SDK's `LineMetrics`; `Font::ink` returns
optional `InkBounds`, preserving empty text and whitespace as no ink.
`canvas.clip_region(rect)` returns a nested guard: its clip intersects the parent,
and dropping it restores the previous clip. Explicit `restore` can report a
host refusal. The draw-frame clip remains the outer bound.

`CommandSender` also opens screens, runs actions, queues a redraw with typed
`RedrawMode`, and closes the submitting plugin's top screen. Screen and action
ids are validated before encoding; each call stages one complete batch or none.

### Reading annotations through interface 7

Use `native::library::Library::annotations` with a `BookRef` and
`AnnotationCursor::start()`. `page.end().total()` counts all annotations for
that book, including bookmarks and highlights without notes. This count is
available from the first page; a whole-library overview reads a first page for
each catalog book. There is no count-only call or catalog has-notes filter.

Annotation rows borrow the caller's reply buffer and include stable per-book
ids, kind, source offsets, progress, timestamps and text/note previews. Each
preview is at most 512 UTF-8 bytes. Continue with `page.end().next()` while
keeping the same book reference. The cursor carries the book's annotation
generation; a catalog reference carries a separate catalog generation. On
`Error::State`, restart that book's annotation enumeration.

For lossless exports, read cut previews with `Library::annotation_text` and
`AnnotationTextQuery::new(id, AnnotationTextKind::Text)` or `Note`. The id is
the annotation id, not the row index. Each part borrows at most 1,024 UTF-8 bytes;
continue with `query.from(part.next_offset().unwrap())` only when the offset is
present. Consume each borrowed part before reusing the output buffer. Text calls
have no annotation-generation input, so a multi-call export must handle concurrent
edits; it does not acquire a snapshot. A caller can recheck annotation generation
and restart if it changed.
