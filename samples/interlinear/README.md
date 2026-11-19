# Interlinear: a translation line under each sentence

*Deutsch: [README.de.md](README.de.md).*

A PaperPaw sample plugin with code, for plugin interface 7. Under each
sentence of a book it shows the sentence's translation in small italics,
line after line under the sentence, or, with the setting *Translation after
the paragraph*, all of a paragraph's translations together after it. The
translations come from a text file beside the book.

## Using it

1. Build it and copy it to the card or the emulator's library (see
   [the template's CODE.md](../../templates/plugin-source/CODE.md):
   `paperpaw-dev build`, `paperpaw-dev build --device emulator`).
2. Put the book's translation file into the plugin's data folder,
   `.paperpaw/plugins/interlinear/` on the card, named as the book without
   its extension and `.txt`: for `alice-in-wonderland.epub`,
   `alice-in-wonderland.txt`. [`example/alice-in-wonderland.txt`](example/alice-in-wonderland.txt)
   translates the first two paragraphs of the sample book into German.
3. Open the book. *Settings → Reading aids* has *Translation lines* (on) and
   *Translation after the paragraph* (off).

The file has one line for each sentence: the sentence as the book has it, a
tab, its translation, in UTF-8. Lines starting with `#` are comments. A
sentence is found by its first 40 letters and digits, so quotes, spaces and
case need not agree with the book. At most 2,048 sentences of a file are
used, translations of at most 1,024 bytes; a long paragraph shows the
translations that fit one answer (3,840 bytes).

## How it works

The plugin subscribes to the reader's text stage `reader.text`
(`docs/abi/page-layers.md`, SDK `paperpaw_sdk::native::page_layers`):

- When a book opens, the reader asks whether the plugin adds to it
  (`TextAsk::Book`). The plugin looks for the book's file, reads it once into
  an index of 8 bytes a sentence and answers `TextAnswer::uses` with a
  signature of its settings and the file. Without a file it answers nothing,
  and the reader never asks it about that book's text.
- Before a paragraph is laid out the reader shows it its text, once while
  the book is open (`TextAsk::Block`). The plugin cuts it into sentences,
  looks each up, reads the translations it finds from the file and answers
  each as `TextAnswer::gloss` (kind `Gloss`, 60 %) or all together as
  `TextAnswer::insert`. The layout makes room under the lines and sets the
  translation there; a page turn asks nothing.

`src/lib.rs` is what runs without a device and is tested on the computer
(`cargo test --lib`); `src/main.rs` is the plugin around it.
