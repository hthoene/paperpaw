### Added

- **A book's own settings for native readers** (#257). `CommandSender::set_scope(Some(book path))`
  now works: while the reading screen is open, settings declared `scoped` (font size, margins
  and the like) take the book's own value where "Only this book" is on, and the global one
  otherwise. The host keeps them where Beta 3's reader did (`books/<book key>/settings.ini`
  in the plugin's data), so the settings saved per book in Beta 3 carry over, and moves and
  removes them with the book. A `book_data` folder name of 16 hex digits (a book key's form)
  is now refused.
