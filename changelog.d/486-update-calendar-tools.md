### Fixed

- **Update no longer stays at *Restarting …*** (#486). Saying no to the
  restart after an update, or a restart that could not start, left the page
  on *Restarting …* for good. Now the page offers *Restart now* again, and
  says so where the device would not restart.
- **The *New* mark goes once the update is installed** (#486). An update
  installed from the card (or another way) kept the *New* mark on Update and
  its offer. At start, an offer the device has reached since is let go.
- **Update says when it could not check your plugins** (#486). Where the
  plugins built in could not be read, an update online read as a plain
  download. It now says that this is checked before the update installs. What
  an interrupted plugin download left behind is removed at the next start, and
  a search the network refused no longer counts as the day's search.
- **Calendar keeps the events of a calendar that fails** (#486). In an
  account with several calendars, one calendar that could not be read lost
  its events, and a subscription answered with a login page (a hotel Wi-Fi)
  lost all of its events. Now each calendar's events are replaced only by a
  whole answer, and a subscription only by a calendar.
- **Calendar keeps the password on an empty field** (#486). Confirming the
  empty password field erased the password, and blanks at the ends of a
  password were dropped. Now an empty answer keeps the password, a password
  is kept as typed, and a calendar is removed only together with its
  password, so a new calendar never sends an old one's password.
- **Tasks notices changes on both sides with every server** (#486). With a
  server that does not tell the new version of a task it was sent, a change
  made on the phone meanwhile was overwritten by the reader's. Now the list
  is read once more after such a sending, and changes on both sides are kept
  as before.
- **The dictionary panel opens for very long words** (#486). A text of more
  than 4,096 bytes without a space kept the panel from opening; it is cut into
  lines now. One lookup provider that fails no longer hides those after it,
  and the indexing that could not plan its next step starts again at the next
  lookup instead of stalling.
- **The browser opens a page that arrived** (#486). Where memory was short
  after a page arrived, the browser stayed at *Loading …*. Now a page without
  a readable title is named after its site, as in Beta 3, and what cannot be
  opened is said. The first bookmark no longer points to
  `https://.m.wikipedia.org/` where no language is set; it is the English
  Wikipedia then.
