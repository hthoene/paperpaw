### Fixed

- **Mail marks a mail read again when you open it** (#486). The setting
  *Mark as read when opened* had no effect; now the server is told, as in
  Beta 3. In a folder the server keeps read-only, the mail still opens.
- **A mail is no longer sent twice** (#486). When the server took the mail but
  the connection broke while saying goodbye, Mail said sending failed, and
  sending again delivered it a second time. Now it counts as sent.
- **Mail keeps checking in the background** (#486). One failed check could
  stop the background check until the reader restarted, and an account with a
  wrong password kept the other accounts from being checked. Now each account
  is checked, and the next round comes as set.
- **A long signature no longer keeps Mail's compose window from opening**
  (#486). Signatures of any length the settings take are used.
- **A large draft in Mail is kept** (#486). A draft with a long text or many
  addresses was taken as no draft and written over; now it opens again. A
  draft file that cannot be opened at all is put aside next to it (Mail says
  where), and the compose window opens empty.
- **News fetches at once when you choose Fetch the news** (#486). The action
  waited an hour first; a rule's fetch now comes at the next good moment
  (waking, going online, charging) and within the hour, as in Beta 3.
- **News feeds can be opened, fetched or removed with the buttons** (#486).
  Holding Select (or the menu button) on a feed offers what a long press
  offers; the list says so below the feeds.
- **News no longer adds a new book with every fetch** (#486). Where a Beta 3
  book with your highlights stayed beside the new one, each fetch added
  another copy (2), (3) and so on; now the book of the last fetch is updated.
- **A news fetch is no longer cut short** (#486). A background fetch that came
  due while you fetched started over from the first feed, and opening a feed
  without a book stopped a fetch of all feeds; now what is asked for meanwhile
  is fetched after it.
- **The news widget is drawn again once a fetch is over** (#486), not after
  every feed, which saves battery on e-ink.
- **News and Wallabag tell servers the real PaperPaw version** (#486), as in
  Beta 3, instead of "PaperPaw/0.1".
- **One large notes file no longer holds up Readwise** (#486). A notes file
  too large for the memory stopped every later export from being sent; now it
  counts as not sent, waits behind the others, and a background run says
  nothing about memory.
