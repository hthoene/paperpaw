### Changed

- **Faster start into the book read last** (#403). The first time a book is opened, the
  reader checks whether its first 80 blocks need letters the reading font lacks. It now
  keeps the result in the book's folder, so later openings and every start skip the
  check. On the X4 each of those block reads took about 10 ms before the first page.
  The list of page counts is no longer rewritten when the book is already its newest
  entry. On the X4 profile of the emulator, a start into the last book now asks for
  3 blocks before the first frame instead of 56. It reads the card 82 times instead of
  154 and writes nothing to it.
