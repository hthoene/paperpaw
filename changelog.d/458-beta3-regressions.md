### Fixed

- **Holding Power while the reader goes to sleep switches it off again** (#458).
  Since the Power escape came, a press held on while the reader went to sleep
  (to switch it off, or a sleeve pressing the button) reset the processor after
  9.5 s instead, and the reader started again and stayed awake. Now the sleep
  takes the escape and switches off the clean way, as in Beta 3.
- **Downloads and servers keep the reader awake** (#458). Store and OPDS
  downloads, the transfer server and a position pushed on sleep run through the
  new network service, which "Sleep after" no longer waited for: a download was
  cut by the sleep. Now a running request or a server on the network puts the
  sleep off again, and going to sleep waits a while for a request under way.
- **A card a computer left marked dirty stays marked** (#458). The reader
  cleared that mark when it kept the free space, so the computer never offered
  its check. PaperPaw sets no such mark itself and now leaves one it finds.
- **Page counts stay right after a pack, a book or the layout changes** (#458).
  The file of a book's page count did not change with the book's size, the
  packs of letters, fonts and hyphenation installed or the layout's version, so
  a count made before was used again and page totals and pages left were wrong.
  And as in Beta 3, a count that promises pages past the book's end is thrown
  away and counted again (once while the book is open).
- **An update the newer X3's display cannot show goes back** (#458). On an X3
  with the UC8279d display, an update on trial whose first screen refresh times
  out now returns to the firmware before it, as Beta 3 did for that display.
