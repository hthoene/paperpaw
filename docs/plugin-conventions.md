# Plugin layout and best practices

How a plugin's folder is laid out, what its files are called and where its
assets go, so that every plugin looks alike and `paperpaw-dev` can check
it. It describes what the loader, the installer and the plugin library
really accept (`crates/paperpaw-core/src/data_plugin.rs`,
`crates/paperpaw-sdk/src/install.rs`, `tools/publish/store-rules.json`,
`paperpaw-dev package`); where a rule is a recommendation and not a
refusal, it says so. The guide to writing a plugin is
[plugins.md](plugins.md), the formats of the data files are in
[data-files.md](data-files.md), the code plugin files in
[ADR 0024](adr/0024-native-code-plugins.md).

Words: **must** is refused by the device, the installer or the plugin
library; **should** is the convention, which PaperPaw's own plugins follow
(and `paperpaw-dev` may warn about); **may** is free.

## Four places, never mixed

| Place | What it is | Who writes it |
|---|---|---|
| The **source tree** | A plugin's crate, or its repository: `Cargo.toml`, `src/`, the files it embeds, tests | the author |
| `plugins/<id>/` on the card | The plugin as readers install it: `plugin.ini`, `code.ppn`, the files the plugin reads on demand | the installer (store, web flasher, USB transfer); replaced as a whole by an update |
| `.paperpaw/plugins/<id>/` on the card | What the plugin writes while it runs (`Env::plugin_dir`): state, caches, downloads | the plugin |
| `.paperpaw/secrets` | Tokens and passwords (`env.secrets()`) | the plugin, through the SDK |

A plugin **must not** write into `plugins/<id>/` (an update replaces the
folder, and files put there otherwise are not what the installer
installed) and **must not** put runtime data anywhere but its
`.paperpaw/plugins/<id>/`. Everything it writes there belongs in the
tables of [data-files.md](data-files.md) (files with a format that
changes carry a version, caches a stamp).

## The canonical trees

### A code plugin's source tree

```text
<id>/                      first-party: plugins/<id>/; own repository: its top
  Cargo.toml               package paperpaw-plugin-<id> (own plugins: any name)
  src/
    lib.rs                 the plugin type; #![no_std]
    <module>.rs            one concern each; tests in <module>_tests.rs
  i18n/
    en.ini  de.ini         one file a language, the same keys in all of them
  how/
    en.txt  de.txt         "How it works", plain text, one file a language
  pictures/                embedded pictures (rare, see Assets)
  data/                    other embedded files (a word list, a table)
  tests/                   integration tests (one file a topic)
  testdata/                fixtures for tests; <name>.v<N> for a persisted format
  plugin/                  own repository only: what readers install (below)
    plugin.ini
    store/store.ini        the plugin library's entry, screenshots beside it
```

Only the folders you need. Names are fixed: a plugin does not call
`i18n/` `lang/`, `how/` `help/`, `pictures/` `images/` or `assets/`.
(PaperPaw's first-party plugins keep their store entry in
`tools/plugin-files/plugins.txt` and the release's catalogue, not in a
`store/` folder of their own.)

### A code plugin as installed

```text
plugins/<id>/
  plugin.ini               [plugin] id, name, description, version
  code.ppn                 the plugin's code (one of CODE_FILES; the installer names it)
  pictures/<name>.png      files the code reads from the card on demand (optional)
  data/<name>              other files read on demand (optional)
  how/en.txt  how/de.txt   shown on the plugin's page in the store
```

At most 64 files, 63 with the code; only `.ini`, `.css`, `.ttf`, `.otf`,
`.ttc`, `.ppfp`, `.txt`, `.md` and `.png` outside `store/`
([Publishing](plugins.md#what-is-uploaded)). Pictures read from the card
are therefore `.png` (no `.jpg` in a package).

### A data plugin

```text
plugins/<id>/
  plugin.ini               [plugin] id, name, description, version, formats
  settings.ini  keymap.ini  styles.css  presets.ini
  marks.ini  menus.ini  actions.ini  icons.ini  setup.ini     (formats 2 and 3)
  i18n/<lang>.ini          translations, also whole languages
  points/<point>.txt       lines for a point of another plugin (formats 3)
  fonts/*.ttf|.otf|.ttc    fonts; fonts/*.ppfp packs named in plugin.ini
  pictures/<name>.png      pictures the settings name (logo, sleep pictures)
  how/<lang>.txt           "How it works" (or <id>.plugin.how in i18n)
```

The kernel reads only the files above (a file it does not read is not an
error; `paperpaw-dev package` says which ones those are). Data files are
at most 64 KiB, `plugin.ini` 4 KiB, and a plugin's data files together
96 KiB (24 KiB of the files of formats 3), its translations 512 KiB.
Data formats are written down in [data-files.md](data-files.md#a-data-plugins-files).

### The reserved names

| Name | Where | Meaning |
|---|---|---|
| `plugin.ini` | top | the manifest; its `id` is the folder's name |
| `code.ppn`, `code.so`, `code-host.*` | top | a code plugin's code; a folder with one is a code plugin's, whether PaperPaw loads it or not |
| `i18n/` | top | translation catalogs, read by the loader for every folder |
| `how/` | top | texts for people; the kernel does not read them; `how/<lang>.txt`, or `how/<plugin-id>/<lang>.txt` in a crate with several plugins |
| `fonts/` | top | fonts and font packs, one folder below it allowed |
| `points/` | top | lines for a point (formats 3) |
| `store/` | top | the plugin library's entry; taken out of what readers install |
| `settings.ini`, `keymap.ini`, `styles.css`, `presets.ini`, `marks.ini`, `menus.ini`, `actions.ini`, `icons.ini`, `setup.ini` | top | data files |
| `pictures/`, `data/` | top | the convention for pictures and other files a plugin reads (not read by the kernel) |
| `tests/`, `testdata/`, `src/`, `examples/` | source tree only | never in what readers install |
| a name starting with `.` | anywhere | refused in a package; `plugins/.<id>.part` and `plugins/.<id>.next` are the installer's |

## Names

| Thing | Rule | Example |
|---|---|---|
| Plugin id | lower-case letters, digits and `-`, 1 to 40 characters, not starting with `-`; the folder's name | `reading-goal` |
| Crate (first-party) | `paperpaw-plugin-<id>` in `plugins/<id>/` | `paperpaw-plugin-reading-goal` |
| Anything the plugin registers | `<id>.<name>`: segments of lower-case letters, digits and `-`, joined by `.` (no `_`, no capitals) | `reading-goal.daily`, `sleep.show` |
| Setting | `<id>.<name>`, or `<id>.<group>.<name>` for a group | `hello.clock.date` |
| Translation key | `<id>.<what>`; the name of a setting is `<id>.setting.<name>`, its help the same with `.help`, the plugin's own `<id>.plugin.name` and `<id>.plugin.description` | `hello.setting.date.help` |
| Plural forms | the key with `.one` and `.other` (or the language's forms) | `hello.greeting.one` |
| Event | `<id>.<name>` | `stats.updated` |
| Files in the folder | letters, digits, `-`, `_`, `.`; none starting with `.`; the whole path at most 100 bytes; lower-case with `-` is the convention | `sleeping-cat.png` |
| Language files | `<lang>.ini` and `<lang>.txt` with the language's code (`en`, `de`, `pt-BR`) | `i18n/de.ini` |
| Data written at run time | in `.paperpaw/plugins/<id>/`; a name says what it holds, a cache has a stamp beside it | `cat-happy-1234.gray` |
| Persisted format fixtures | `<name>.v<N>` in `testdata/`, one per version a plugin must still read | `bookmarks.txt.v1` |

Do not use em or en dashes in any text of a plugin (`paperpaw-dev`
checks it).

## Assets

Pictures and other files a plugin needs come from two places:

| | On the card (`plugins/<id>/pictures/…`) | Embedded in the code (`include_bytes!`) |
|---|---|---|
| Where it lives | the plugin's folder on the card, installed with it | inside `code.ppn` |
| X4 | read from the card when needed; costs nothing in flash | written to flash with the code, into the plugin room, which is limited (ADR 0024) |
| X4 Pro | read from the card when needed | the code is read into PSRAM with everything embedded |
| Android, emulator | read from the card or storage | part of the library |
| Memory | decoded row by row from the card (`image::picture`), at most a few rows in RAM | the same, from flash |
| Update | replaced with the plugin | replaced with the code |
| The user can replace it | yes (a file on the card) | no |
| Use for | every picture over about 4 KB; anything several of; photos | a logo or an icon of a few KB the plugin is useless without; a table the code indexes |

The rule: **a code plugin's pictures go on the card, in `pictures/`**,
and the code reads them with `image::picture` from `plugins/<id>/pictures/<name>.png`
and copes when a file is missing or cannot be read (a short text instead
of a picture). Embed only what the plugin cannot work without and keeps
below about 4 KB, and say why in a comment above the `include_bytes!`.
Every embedded byte is paid by the plugin room of the X4 and every
download of the store.

First-party plugins that are built into the firmware (the sleep screen's
cat, the reading fonts, the transfer pages) embed their files, as the
firmware has room for them and no installer has run on a first start; they
say so in their `Cargo.toml`, with the reason, and `paperpaw-dev package`
leaves them alone:

```toml
[package.metadata.paperpaw]
embedded-assets = "built into the firmware: the sleeping cat is drawn before ..."
```

A plugin of the store whose files are on the card names them in the same
table, as globs from the plugin's folder, and the store's package holds
them (`tools/plugin-files/catalogue.py`; only the kinds a package may
hold: a page of HTML is a `.txt`):

```toml
[package.metadata.paperpaw]
card-files = ["pictures/*.png"]
```

PaperPaw's own plugins of the store are also part of the apps (Android,
the desktop, the emulator), where nothing installs a folder. They carry
the same files in the code for those, behind a default feature
(`embedded-pictures`, `embedded-quotes`, `embedded-page`), and read the
card's file first. The plugin files for the readers (`tools/plugin-files`)
are built with `default-features = false`: they carry none, so their
`.ppn` is smaller, and their package brings the files. A plugin of your
own that is only ever installed needs no feature: it reads the card, and
copes where the file is missing (a short text where a picture would be).

### Pictures for e-ink

| | Recommendation |
|---|---|
| Format | PNG, grayscale 8-bit (`.png` is the one kind a package may hold); not interlaced (the decoder refuses it). JPEG and GIF read fine from the card of a reader, but a package may not hold them |
| Size in pixels | at most the screen the picture fills: 480 x 800 for the X4 and X4 Pro; a logo or icon in a box, 160 x 160 or less. The decoder scales larger pictures down while it reads, so a larger file only wastes room |
| File size | below 60 KB for a full-screen picture, below 10 KB for a logo or an icon (PaperPaw's largest is 75 KB) |
| Tone | the X4's panel is darker than a screen: lighten photos (gamma about 1.3), draw line art with solid black lines, no thin gray ones |
| Transparency | PNG alpha is kept for pictures drawn over the page (`picture_with_alpha`); elsewhere it comes out over white |
| Names | what it shows, `sleeping-cat.png`, `happy.png`; numbered where they are a sequence (`1.png`) |
| Licence | pictures are published with the plugin: yours, or with a licence you name in a `.txt` beside them |

The Workshop's `convert_picture` makes such a PNG (fitted, toned, gray,
small).

## Translations and help texts

- Every user-facing text comes from `i18n/<lang>.ini`; **English and
  German** both, with the same keys (a test for that belongs in every
  plugin, as the sample has it). Other languages are `i18n/<lang>.ini`
  beside them, a language of its own and never a draft of another.
- `<id>.plugin.name` and `<id>.plugin.description` are in every catalog.
- *How it works* is `how/<lang>.txt` (plain text, paragraphs apart by an
  empty line, at least 120 characters, `paperpaw-dev` warns about a short
  one), registered with `Manifest::how` through `include_str!`. A data
  plugin gives it as `<id>.plugin.how` in its catalogs instead. A crate
  that ships several plugins has one folder for each:
  `how/<plugin-id>/<lang>.txt`. (The EPUB, HTML, FB2, Markdown and CBZ
  formats are five plugins, each a crate of its own, `plugins/format-*`,
  over the shared conversion of `crates/paperpaw-epub-family`.)
- A crate with several parts that are one plugin (`widgets`) has one
  catalog a language, `i18n/<lang>.ini`, not one a part.
- Put the same `how/` files into what you upload: the store shows them.
- Keep a plugin's catalogs small: they are embedded and loaded when the
  plugin shows text; a catalog over 64 KiB is left out by the loader (a
  data plugin's), and the Workshop takes 128 KiB per language file.

## Settings

- Declared in a `static` list (`r.add_settings`), never built at run
  time; ids as in *Names*; every setting has a translation key for its
  name and, if a word is not enough, a `.help`.
- A setting that stores something large (a list, a text) is not a setting:
  it is a file in `.paperpaw/plugins/<id>/`.
- Defaults must work with nothing set (a first start, a restored
  device); a setting that keeps the device safe is never set by a data
  plugin.

## Data a plugin writes

- Only under `.paperpaw/plugins/<id>/` (`Env::plugin_dir(id)`); secrets
  only in `env.secrets()`.
- Files that must survive an update have a documented format and, once
  that format changes, a version and a fixture of the old one
  (`testdata/<name>.v<N>`) that a test reads.
- Caches carry a stamp (what they were made from) and are made again when
  it differs; a cache is never the only copy of anything.
- Add every file to [data-files.md](data-files.md) in the commit that
  introduces it.
- How to write a file so that a power cut leaves the old one or the new
  one, not half of either: TODO, a convention follows from the kernel.

## Memory, flash and the store

The 10 KB rule is PaperPaw's own, for what is added to the firmware; the
4 KB for embedded assets above is for every code plugin, which pays
for them in its own file.

| Budget | Rule |
|---|---|
| Heap (X4: 256 KiB for everything) | keep lists in files; `static` specs; read files a piece at a time; `try_reserve` large buffers and cope; see [Memory](plugins.md#memory) |
| Code plugin file | `opt-level = "s"`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`; no float parsing (`str::parse::<f32>`), no `{:?}` of strings; the store takes a build of at most 4 MiB |
| Embedded assets | below about 4 KB each; the rest on the card |
| Room for code plugins on the X4 | 2,273,280 bytes with PaperPaw's layout (a full flash by USB or the web flasher), about 640 KB after an install by CrossPoint's SD installer (measured on a device); the X4 Pro has PSRAM |
| First-party rule | an addition built into the X4 firmware that grows it by more than 10 KB becomes a store plugin instead |
| Data plugin | data files 64 KiB each, 96 KiB in all (24 KiB for formats 3) |
| Package | at most 64 files (63 with code), 20 MB zipped, 40 MB unpacked, paths of 100 bytes |
| Store entry | id 40, name 60, summary 160, description 2000 characters; up to 5 tags and 4 screenshots (500 KB each) |

## Checklist before publishing

- [ ] `plugin.ini` has `id`, `name`, `description`, `version`; the id is
      the folder's name and fits *Names*.
- [ ] Every registered id, setting key and translation key starts with
      `<id>.`.
- [ ] `i18n/en.ini` and `i18n/de.ini` have the same keys; the plugin's
      name and description are in both.
- [ ] `how/en.txt` and `how/de.txt` exist and say what the plugin does and
      where it is found.
- [ ] Pictures are PNG, gray, not interlaced, sized for the screen; those
      over 4 KB are on the card in `pictures/`, not embedded.
- [ ] Nothing is written into `plugins/<id>/`; every file written lives in
      `.paperpaw/plugins/<id>/` and is in data-files.md.
- [ ] No file name starts with a dot; at most 63 files with the code;
      only the kinds a package may hold.
- [ ] The code plugin's build is small (`opt-level = "s"`, no float
      parsing) and its `.ppn` fits the plugin room of the X4.
- [ ] `paperpaw-dev check` and `paperpaw-dev package` are clean (also
      `paperpaw-dev package path/to/crate` for a code plugin's source).
- [ ] No em or en dashes in any text.
