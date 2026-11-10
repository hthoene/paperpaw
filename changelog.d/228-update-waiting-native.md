### Changed

- **Update on interface 7: an update that waits for plugins** (#228). When the update server names a newer
  PaperPaw but plugins you use have no version for it yet, the update page says which plugins it waits for
  (and which can never come along), and offers *Keep waiting*, *Update anyway* (the offer then says what
  stops working) and *Check again*. PaperPaw asks the server once a day at a cheap moment; if you have not
  opened the update page within a week, it opens once by itself. The file `waiting.txt` is the same as
  before. An update you download anyway is not asked about again.
