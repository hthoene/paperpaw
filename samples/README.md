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

A plugin with code to start from is made by `paperpaw-dev new --github <id> --code`:
an action in the tools, a screen, a setting and an event.
[Writing plugins](../docs/plugins.md) walks through it; try it in the emulator
with `paperpaw-dev build --device emulator` and
`paperpaw-emu --library dist/emulator --trust-code`.
