### Fixed

- **A book's cover no longer vanishes from Home for good after you read it a
  little** (#308). Home asked the library for the cover with the list it had
  read a moment before; when the book was read in between (its progress
  changed as the reader closed), the library said the list was out of date and
  Home took that for "this book has no cover", kept it for the whole session
  and never asked again. Now an answer that says nothing of the book (list out of
  date, no room, card hiccup) shows the card or the shelf without the cover and
  asks again at the next minute, with the list read anew.
- **Home no longer loses its whole card when the heap is low** (#308). The
  library's cover call needs about 30 KiB while it makes a cover; where there
  was no room the refusal ended Home's draw (`home: call 10 ended with -4`, six
  times with the store and Wi-Fi open) and left the card and the shelf blank.
  Now Home draws them without the cover and tries the cover again at the next
  minute.
