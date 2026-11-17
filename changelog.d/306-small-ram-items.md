### Changed

- **Four small memory items at start and on gray frames** (#306). The gray frame is drawn in the memory of the band frame and the band is made again in it, so a gray pass allocates nothing and leaves no hole; `paperpaw.ini` is kept as read, not as a second copy beside it; the kernel's lists give back their reserve through the helper that never aborts; the hyphenation index of a table on the card is kept in 16-bit steps (about 40 % less: 3.5 KB for German).
