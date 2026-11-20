### Changed

- **Faster start into the book read last** (#403). The reader now checks whether a
  book needs letters the reading font lacks after its first page is shown, not before.
  The check reads the book's first 80 blocks, and on the X4 each read took about 10 ms
  before the first frame. The list of page counts is no longer rewritten when the book
  is already its newest entry. On the X4 profile of the emulator, a start into the last
  book asks the document service for 3 blocks before the first frame instead of 56. It
  also reads the card 82 times instead of 154 and writes nothing to it.
