### Fixed

- **The store no longer hangs on a full card** (#495). When the memory card
  could not be written and a plugin's page could not be asked for, the store
  tried the same plugin again and again and the device stopped responding.
  The queue now waits until the card has room again; a plugin that asked for
  others is told they were not installed.
- **Calendar events stay while one calendar does not answer** (#495). On the
  first sync after the update, the events of a calendar that failed right
  then disappeared until it answered again. They now stay; the first sync in
  which every calendar answers replaces them.
- **The background mail check ends early without a network** (#495). When
  the device had no network (or no clock, or no memory), the check still
  tried every other account, the radio on for each. It now stops and tries
  again at the next check.
