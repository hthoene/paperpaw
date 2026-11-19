# Read-along: mark the sentence being read aloud

*Deutsch: [README.de.md](README.de.md).*

A PaperPaw sample plugin with code, for plugin interface 7 (#377). While a
book is read aloud ("Read aloud" in the reader) it draws a frame around the
words being read, or a thick line under them, on each line of the sentence;
the page is drawn again for each sentence in one fast refresh. Nothing is
drawn, asked or kept while the book is not read aloud.

## Using it

1. Build it and copy it to the card or the emulator's library (see
   [the template's CODE.md](../../templates/plugin-source/CODE.md):
   `paperpaw-dev build`, `paperpaw-dev build --device emulator`).
2. *Settings → Reading aids → Mark the sentence read aloud*: *Frame*
   (default), *Thick underline* or *Not marked* (the plugin then does not
   follow reading aloud at all). The reader's own thin underline has its own
   setting (*Underline the sentence*) and its page turns follow the setting
   *Turn pages*: the plugin leaves both to the reader.

## How it works

The plugin subscribes to the reader's layer stage `reader.layer`
(`docs/abi/page-layers.md`, SDK `paperpaw_sdk::native::page_layers`):

- When a book opens the reader asks whether the plugin draws on its pages
  (`LayerAsk::Book`). The plugin answers `LayerAnswer::uses_reading_aloud`
  with a signature of its style, or nothing for *Not marked*.
- For each sentence read aloud the reader asks the page on show again with
  the words being read (`LayerPage::spoken`). For each line the plugin takes
  the stretch from the first to the last word read (`Spoken::holds`, the
  words' boxes of `LayerLine::words`) and answers one `LayerAnswer::rect` (a
  frame, no fill) or `LayerAnswer::segment` for it. A layer is drawn over the
  text, so a mark never fills a box: it would hide the words.
- A page asked without words being read (the first ask of each page, and
  when reading aloud ends) is answered with nothing, which clears the marks.

The host's speech reports a sentence at a time (`event.spoken`), so a
sentence is marked, not the word: a word-by-word mark needs the host to say
where in a sentence the voice is, which interface 7 has not.

`src/lib.rs` is what runs without a device and is tested on the computer
(`cargo test --lib`); `src/main.rs` is the plugin around it.
