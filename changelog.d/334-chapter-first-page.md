### Fixed

- **Reader: pages left in a chapter that begins a file** (#334). A chapter
  that begins a file is on the first page of that file, so the status lines
  no longer count one page too many for the chapter before it, and the first
  chapter of a book is found on the first page. Found in a fork by Alexandre
  Cormier ([alexcormier/paperpaw@958537c](https://github.com/alexcormier/paperpaw/commit/958537c4a2751a7dbf75175b51511790a6cd22b4)).
