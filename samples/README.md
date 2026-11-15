# Samples

`library/` is the default storage of the desktop emulator
(`cargo run -p paperpaw-emulator`); pass `--library <dir>` to use another
folder. The emulator writes reading positions and page indexes to
`library/.paperpaw/`, which is ignored by Git.

## Books

Both books are in the public domain and come unchanged from
[Project Gutenberg](https://www.gutenberg.org), including its licence text,
each as plain text and as EPUB:

| Files | Book | Source |
|------|------|--------|
| `library/books/alice-in-wonderland.txt`, `.epub` | Lewis Carroll, *Alice's Adventures in Wonderland* (English) | [eBook #11](https://www.gutenberg.org/ebooks/11) |
| `library/books/die-verwandlung.txt`, `.epub` | Franz Kafka, *Die Verwandlung* (German) | [eBook #22367](https://www.gutenberg.org/ebooks/22367) |

## Plugins

`plugins/large-print` is a data plugin: larger text in books and menus,
with its name translated to German. To try it, copy it into
`library/plugins/` (or the `plugins` folder of a device) and restart.

`plugins/airy-theme`, `plugins/theme-calm-ink` and `plugins/theme-paw`
are themes as data plugins: new defaults for the appearance settings.

`plugins/reading-nook` is a home screen as a data plugin. Its
`settings.ini` gives the home screen new defaults, shown until the
reader arranges it. Its clock comes from the widget pack (`widgets`).
Switch that on in *Settings → Plugins* first. On the X4 and X4 Pro,
install it from the store.

`hello-plugin` is a plugin with code to start from: an action in the
tools, a screen for touch and buttons, a setting, an event and
translations. [Writing plugins](../docs/plugins.md) walks through it; try
it with `cargo run -p paperpaw-emulator --features hello`.

`interlinear` writes between a book's lines ([ADR 0032](../docs/adr/0032-writing-between-the-lines.md)):
a toy translation above each line, starting where each sentence on the
line starts, which a tap (or Select, chosen with the buttons as links
are) opens whole; and a translation after each paragraph. Try it with
`cargo run -p paperpaw-emulator --features interlinear`.

`frame-test` runs in real time ([ADR 0030](../docs/adr/0030-real-time-screens.md)):
a moving pattern at a fixed pace, drawn with the blit, and work done for
every frame as hot code and as ordinary code, each measured and shown
with the frames a second and the heap free. Try it with `cargo run -p
paperpaw-emulator --features frame-test`.
