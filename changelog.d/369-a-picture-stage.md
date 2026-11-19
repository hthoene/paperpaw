### Added

- **Plugins can shape how pictures look: the picture stage `core.picture`** (#82, #369). A plugin
  that subscribes to the new stage (at most two on a device) is asked once for each picture the
  host prepares, with where it is shown, its size and a histogram of its grays, and may answer a
  tone curve of 256 grays and whether it is drawn in dots or in grays. It sees no pixels and is
  never asked at a draw, so page turns cost nothing more. The SDK has it as
  `native::image::picture` (`Asked`, `Tone`, `Place`). The host asks for every picture it
  draws by an image id and says where it is shown: a picture on a book's page (with the book's
  path), a page of a comic, a cover, a small picture, a picture of the sleep screen, or a
  picture a plugin prepared for itself. A comic's panels take their page's answer. Without a
  subscriber nothing is read and no plugin runs (`docs/abi/document.md` 5).
  Appended to interface 7; plugins built before run unchanged.
