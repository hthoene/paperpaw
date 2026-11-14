### Changed

- **The native dictionary waits for lookups that answer later, and runs as a native plugin with the native reader** (#228). Where a lookup service answers later (it must fetch), the panel says "Looking up …" as Beta 3 did, shows the answer when it is ready on the page it showed, and stops waiting after 20 seconds. Builds that link the native reader list the native dictionary in place of its interface-6 form, with its settings kept in the program.
