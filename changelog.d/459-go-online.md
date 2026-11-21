### Fixed

- **Offline mode only said "no network" in seven plugins** (#459). Where Offline mode holds back what the reader started, they ask once whether to go online for it again, as in Beta 3, and *Go online* does it again: Calibre (connecting), OPDS catalogs (a feed, *Sync*), Progress sync (*Sign in*, *Create account*), News (fetching), wallabag (*Fetch articles*), Browser (a page) and Joplin (*Send*). Background work stays quiet (calibre 1.0.14, opds 1.0.6, kosync 1.0.14, news 1.1.3, wallabag 1.0.10, browser 1.0.11, joplin 1.0.10).
