### Fixed

- **A damaged file of wrong tries counts as all of them used** (#257). When the lock started and the file that keeps the count of wrong codes was cut, damaged or too long (a power cut while it was written), it was read as none. It now counts as five, so the next try waits half a minute and the count is forgotten after it. Where there is no file at all, the count is still none.
