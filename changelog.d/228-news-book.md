### Changed

- **News on interface 7, third slice** (#228). A fetched feed is made into the page of its book in small pieces over the storage calls (at most 4 MiB of feed, the page written in calls of 4 KiB), refusing instead of aborting when memory runs short; a feed without articles leaves no page. The page and the result are the same as before. The plugin still runs as before until the native plugin takes over.
