### Changed

- Fifteen first-party plugins with fixed declaration tables (bluetooth, file-manager, help, home, keyboard, lock, menu, opds, power, reader, settings, aids, news, vocabulary, widgets) also keep the table of the ids they declare in the program, so the kernel no longer copies them onto the heap at the start (about 5 KiB less at idle on the X4).
