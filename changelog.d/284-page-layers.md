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
