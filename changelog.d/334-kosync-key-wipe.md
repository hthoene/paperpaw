### Fixed

- **KOSync: the account key leaves no copy in memory** (#334). The key
  written into a request body or read from the account file was left in
  freed memory when the text had to grow or carried escapes; it is now
  made in one block and wiped.
