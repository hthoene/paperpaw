### Changed

- **The screen lock is a native interface-7 plugin** (#228). The lock screen (PIN, pattern, password, setting up the code, the kinds list), locking at start and when the device wakes, *Lock now*, the wait after five wrong tries and the code kept among the secrets work as before, with every allocation fallible, the codes wiped when done with and no code ever printed; the interface-6 plugin is gone. The touch lock, the setting's immediate revert and the end of the wait without an input wait for host pieces (see `plugins/lock/PORTING-7.md`).
