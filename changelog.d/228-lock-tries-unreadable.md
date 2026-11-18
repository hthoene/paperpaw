### Fixed

- **An unreadable count of wrong tries no longer counts as none** (#257). When the lock started and the card could not tell how many wrong codes were kept, the count began at zero. It now begins as used up, so the next try waits half a minute, after which the count is forgotten as usual.
