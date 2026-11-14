### Changed

- **On the Xteink readers book text is now set in the menus' font (PaperPaw Sans) unless you
  choose another font; Literata is a font of the card** (#258). The firmware no longer carries
  Literata: 257 KB less flash on the X4. The plugin *Literata* (`literata-styles`, before
  *Literata bold and italic*) now brings the regular face too, version 1.1.0: with it on the
  card, choose Literata in Settings, under Reading, as Font. Until then Literata is not
  offered, and a card with the plugin of before keeps its bold and italic closed rather than
  mixing them into the menus' font. Other devices keep Literata as the book's font.
- **Bold and italic book text comes from the family of the regular book text only.** A bold
  or italic face a plugin proposes, or of a chosen family, is used where regular book text
  is in the same family; else the regular face stands in, as where no bold is bound. A
  binding you make yourself under `[fonts]` is taken as given.
- **Bold book text in a font without a bold face is drawn bolder** (faux bold): the regular
  face with every letter one pixel wider for every 24 px of its size, in the same room, so
  headings and bold words stand out on a reader that has no Literata on its card.
- **A font family is offered for reading and for the status lines only where it has a
  regular face that is there.** A family of bold and italic alone is no longer listed.
- **A font chosen stays chosen while the card cannot say whether its file is there** (busy,
  a read fault); only a file that is gone gives the choice up, also for a font in use whose
  plugin is removed. Literata chosen before, on a card without the plugin, stays in
  `paperpaw.ini` for the start that finds it, and the log names the font and the missing
  plugin rather than an invalid value.
- **A pack that extends a font (a script's letters, larger sizes) is open only while that
  font is**, so packs made for Literata take no memory while another font sets the book.
- **paperpaw-plugin-fonts-default:** new `LeanFontPacks` (the fonts of a lean build, in any
  build) and `LITERATA_ON_CARD`; a lean build registers `fonts-default.literata` as a face in
  that file and binds `fonts-default.source-sans` to the reading role. `paperpaw-app`'s
  `Fonts::Packs` registers the lean set. **paperpaw-sdk** (kernel): new `Fonts::family_there`.
