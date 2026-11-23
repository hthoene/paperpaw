### Fixed

- **File sync no longer deletes server files when a folder has many hidden
  files** (#486). Hidden names on the card could push files out of the
  listing, and the sync then took those files for deleted on the card and
  deleted them on the server. Hidden names no longer count, and a folder
  that cannot be listed whole is left as it is.
- **File sync tells the true limits and recovers from a sync that broke
  off** (#486). The help and the messages now say 200 files and 64
  folders, as the sync allows. A sync that fails halfway now says so, ends
  cleanly and no longer blocks the next automatic sync. Last sync only
  shows syncs that went through. Progress redraws only the line below the
  pairs.
- **File sync keeps passwords as typed** (#486). Spaces at the start or end
  of a password are kept. A server address for another server takes the
  old password away, so it is never sent there.
- **File sync keeps listing trash files it could not remove** (#486). Such
  a file used to drop off the list and stay on the card for good. It is now
  removed at a later sync. A listing the reader could not save whole no
  longer counts as an empty folder.
- **OPDS moves old saved passwords out of the catalog list** (#486).
  Passwords from an older version now go to the passwords and keys when the
  list is read. A new password that cannot be saved no longer wipes out the
  old one.
- **OPDS browsing no longer breaks a running background sync** (#486).
  Leaving a catalog no longer removes the sync's files. A new list starts at
  its first row, and download progress redraws only the line below the
  list.
- **OPDS gives clearer errors** (#486). A full card, a file that is too
  large or a busy network now says so instead of "check the Wi-Fi". A
  public catalog no longer fails when the passwords cannot be read. Back
  stops a search that is still loading.
- **Progress sync respects Offline mode again** (#486). In Offline mode
  nothing is sent or fetched, and the position of the book you left is sent
  once the network is allowed, as in Beta 3. A position that fails to send
  is tried again.
- **Progress sync accepts only http:// and https:// server addresses**
  (#486). Any other address is refused with a message, and the account
  screen no longer gets stuck on "Connecting". A password store that is
  busy for a moment no longer signs you out. "Go there" says when the book
  cannot be opened at that place. Sign-out says when the account's key
  could not be removed. A book's fingerprint is read once, not each time
  the position is fetched or sent.
- **Highlights sharing moves KOReader's notes with a moved book** (#486).
  Moving or renaming a book takes its `.sdr` folder along again, as in
  Beta 3, so a later question cannot remove highlights by mistake. Books
  with more than 128 changes are now exchanged on the X4 too.
- **Wireless library handles books that cannot be received** (#486). A
  book that does not fit on the card, or that the card refuses, ends the
  connection with a clear message instead of staying on "Receiving". A
  password that cannot be saved is used until the screen closes, so you are
  not asked for it again and again. "Delete from device" in Calibre keeps
  the reader's highlights and notes, as in Beta 3.
