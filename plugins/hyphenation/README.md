# Hyphenation

This folder holds the patterns of American English (`patterns/`). English
is a data plugin the firmware and the web flasher put on the card
(`crates/paperpaw-default-packs`, about 45 KB), so a fresh install hyphenates
English books. German is a store pack of its own (`hyphenation-de`, about
354 KB) and is not installed by default.

Every other language is a plugin of its own in `plugins/hyphenation-<code>`
(French, Spanish, Italian, Dutch, Portuguese, Swedish, Danish, Norwegian,
Polish, Russian, Ukrainian). Each is a store plugin
([`tools/plugin-files`](../../tools/plugin-files/README.md)) with a version
of its own, one language (Norwegian: Bokmål and the tag `no`) and the same
few lines of code. A language is added by copying one of them, changing
its `patterns/`, `build.rs` and texts, and listing it in
`tools/plugin-files/plugins.txt`, the workspace and `paperpaw-app`.

## Why code plugins, not data plugins (Beta 3; today they are data plugins)

The two sections below describe the Beta 3 design, where the tables were code
plugins. Since then the packs are data plugins, built by
[`tools/hyphenation-packs`](../../tools/hyphenation-packs), which the kernel
reads through `text.hyphenate`; the sizes are those of the Beta 3 plugin files.

A pattern table is read in place: hyphenating a word looks up about 10 to
15 keys per letter, each by binary search. That is cheap in memory only
while the table lies in flash, as a code plugin's constant data does on the
X4 (the plugin is mapped into flash and read through the cache, so it costs
no heap). A data plugin would keep the table on the card, and there the
same lookups would cost either RAM or many small reads:

- Loaded into RAM, a table takes its whole size: 2 KB (Italian)
  to 109 KB (Dutch) and 252 KB (Norwegian), against a heap of 256 KB.
  Only the smallest would fit a few KB.
- Read from the card as needed, a word takes on the order of a thousand
  small reads (an estimate, not measured on a device), a second or more
  at the card's speed; a page has dozens of hyphenation candidates.

So the languages are code plugins with `&'static` tables, one per
language, so that a reader installs only what they read.

## Sizes

Compiled tables (`paperpaw-hyphen`, one file per language) and the plugin
files built for the X4 (`tools/plugin-files`, `paperpaw-sign pack`, measured):

| | Table | Plugin file |
|---|---|---|
| Italian | 2.3 KB | 22 KB |
| Portuguese | 2.7 KB | 23 KB |
| Danish | 7.9 KB | 28 KB |
| French | 12.4 KB | 32 KB |
| Polish | 37.4 KB | 57 KB |
| Swedish | 38.5 KB | 59 KB |
| Ukrainian | 47.6 KB | 68 KB |
| Spanish | 49.1 KB | 69 KB |
| Russian | 74.9 KB | 95 KB |
| Dutch | 108.9 KB | 129 KB |
| Norwegian | 251.9 KB | 272 KB |

A plugin file is its table plus about 20 KB: 15 KB of code (its copy of
the SDK parts it uses) and 4 KB of texts. All twelve together take
876 KB of flash; the X4's app partition has room for them behind a firmware
of 1.7 MB, but nobody needs more than a few. Heap per language: a table
descriptor of 32 bytes and one entry in the registry, plus a few hundred
bytes while a word is hyphenated, as for German and English. A word takes
5 to 13 microseconds on a computer.
