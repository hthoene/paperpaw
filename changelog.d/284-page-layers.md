### Added

- **Page layers for plugins: translations, glosses and drawing on the reader's pages** (#284,
  #261). Two new stages of the reader take plugins in again that Beta 3 had as text filters and
  page layers. With `reader.text@1` a plugin sees the text of each paragraph before it is laid
  out, once while the book is open, and may ask for room above and below its lines, replace
  words as shown, set glosses, ruby or phonetics over or under words (a sentence's translation
  runs under the sentence, line after line) and insert small blocks after the paragraph. With
  `reader.layer@1` it is shown each laid-out page once, with the places of its lines and words,
  and answers boxes, lines and text the reader draws over the page whenever it is drawn. Page
  turns ask nothing they asked before; without such a plugin nothing changes. The SDK has them
  as `native::page_layers` (`TextAsk`, `TextAnswer`, `LayerAsk`, `LayerAnswer`), and
  `Stage::run_from` asks one subscriber of a stage again. The sample `samples/interlinear`
  shows a translation line under each sentence of a book from a text file beside it
  (`docs/abi/page-layers.md`). Interface 7 is unchanged: plugins built before run as they did.
- **Building blocks for read-along, windows at a word and split pages** (#284, #377, #378,
  #379). A page layer may follow reading aloud: it is asked again for each sentence, with the
  words being read (`LayerAnswer::uses_reading_aloud`, `LayerPage::spoken`), and marks them as
  it likes. It may give the book's text only a part of the page (`LayerAnswer::region`, in
  thousandths of the text area) and draw a second text, notes or a picture in the rest; its
  lines say where in the book they are, so the plugin follows page turns. It may be shown taps
  (on its part of the page only, or all) and key actions before the reader acts on them
  (`LayerAnswer::input`, `LayerAsk::Tap`, `LayerAsk::Key`), take them (`LayerAnswer::taken`) and
  draw its layer anew, the page drawn again in one partial refresh. The lookup slots get
  the looked-up words' box on the screen (`anchor`), so a dictionary window can open next to
  the word.
