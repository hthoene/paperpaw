### Fixed

- **The setup offer keeps what was chosen when it is left with Back** (#486).
  Plugins chosen on the setup's offer were forgotten when the page was left
  with Back instead of *Done*; they are now added as *Done* adds them, as in
  Beta 3.
- **Updates go on after a plugin's page was looked at** (#486). Opening or
  leaving a plugin's page while *Update all* or the setup fetched plugins
  could stop the queue until the next restart, or leave the page saying
  *Loading plugin* for ever. The page and the queue now fetch apart, and a
  page left behind no longer leaves its download on the memory card.
- **A plugin page that cannot be fetched says why** (#486). When the request
  could not even be sent, the page kept saying *Loading plugin*, or said the
  library no longer offers the plugin; it now says there is no connection,
  and the queue goes on with the next plugin instead of waiting.
- **Names in the setup's summary** (#486). A plugin whose page did not come
  showed no name in the summary ("Could not be added: , ."); it is named
  again, and *Update all* no longer shows a notice without a name for it.
- **Waiting updates go on when the store is opened** (#486). Updates left
  waiting by a restart or a missing connection did not go on by themselves;
  they now go on when the store is opened, as in Beta 3.
- **The setup's plugins ask for nothing more** (#486). After a plugin the
  setup added in the background was installed, the store asked whether to
  fetch the plugins it requires; as in Beta 3 it no longer does.
- **Lists show the progress of downloads** (#486). While a plugin was
  downloaded, its row in the lists did not show how far it was, and *Update
  all* did not mark the plugins queued; the rows now follow, and the list is
  drawn again only when a mark changed.
- **The store's start opens faster when it comes back** (#486). Coming back
  to the start no longer reads the whole list from the memory card again
  unless a plugin came, went or the list changed.
- **Why a download failed is said right** (#486). A connection that went
  while downloading was said as a plugin that could not be installed, and a
  host that refused the install as a plugin that could not be downloaded; a
  memory card that did not take the files now says so too.
- **The community source is left after a download in the background**
  (#486). When an install from a community source ended after the store was
  closed, the store still listed that source the next time it was opened; it
  lists PaperPaw's library again.
- **A search or an address that is too long says so** (#486). Typing more
  than the store reads did nothing at all; a notice now says the text is too
  long.
