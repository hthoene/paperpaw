# 0032. Writing between a book's lines

- Status: Accepted; annotations reserved in plugin interface 4, carried
  out in interface 7 (#284): the layout sets them, and the reader's stages
  `reader.text@1` and `reader.layer@1` take text filters and page layers
  (`docs/abi/page-layers.md`)
- Date: 2026-10-25

## Context

A community author writes an interlinear reading plugin: a small
translation above each line of a book, a parallel text with a
translation after each paragraph, words that open their gloss when
tapped. Such plugins need what only the layout and the reader can give:
room between the lines that pages break with, text that is shown but is
not the book's, the page's words where they are drawn, and taps on what
the plugin drew. Ruby, furigana, phonetics and glosses set over single
words need the same room, placed by the layout. All of it crosses into
plugins, so it has to come with an interface raise (ADR 0026).

## Decision

In plugin interface 4:

- **Room above lines**: a text filter sets `Block::leading_above`
  (hundredths of an em, at most four). The layout adds it to each of the
  block's lines above its letters, so baselines move down and pages
  break with it; `PageLine::leading` and `LineBox::leading` tell it.
- **Inserted blocks**: a text filter inserts blocks after the one it
  gets (`Block::insert_after`, at most four, 4 KiB of text in all). They
  have no place in the book: their start and end are the block's end,
  their lines (`PageLine::inserted`) no word places. Positions,
  progress, bookmarks, highlights, search and the page's words stay the
  book's. A page never breaks inside them: where they do not fit below
  the block, its last line goes on to the next page with them, if they
  fit there whole; else what fits stays and the rest goes. Inserted text
  has no place of its own a page could start from, so it cannot go on
  over a page break: what a plugin inserts should be short (a gloss, a
  note, the translation of a sentence), not longer than a page.
- **Words for layers**: a page layer that asks (`PageLayerSpec::words`)
  gets the page's word boxes (`PageView::words`), the reader's own,
  measured once a page and only where a layer asks.
- **Input on layers**: a layer says where its items are
  (`PageLayerSpec::items`, from the page as drawn). A tap on one, or
  Select on one chosen with the buttons among the page's links, comes
  to its plugin as `PAGE_LAYER_TAPPED` with the item's id; the reader
  does nothing else with it. Everything else stays the reader's.
- **Annotations, reserved**: `Block::annotations` (`Annotation`: a byte
  range of the block's text, its text, its kind (`AnnotationKind`: ruby,
  phonetic, gloss), its size) and `PageLine::annotations`
  (`LineAnnotation`: where the layout set one, its text, kind and size)
  are there, empty. The planned use: formats (EPUB `<ruby>`) and filters
  fill `Block::annotations`; the layout reserves room above the lines
  that have any (as `leading_above` does), places each over the start
  and end of its range on the line, and the reader draws them. That
  comes without a raise: the types do not change.

## Consequences

- Interlinear and parallel readings, glosses and notes that open are
  plugins, on every device, by touch and with the buttons.
- Inserted text costs memory only while its block is laid out, and is
  bounded per block.
- A filter that inserts much makes the book longer in pages; its
  signature lays pages out anew when it changes, as for any filter.
- What the layout does with annotations is decided when it is built:
  how much room a line takes for them, whether they may overlap their
  neighbours, how they break with the line.
