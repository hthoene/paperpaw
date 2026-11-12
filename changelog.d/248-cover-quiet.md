### Changed

- **Covers and conversions wait for a quiet second** (#248). The library's covers are made, converted and started only once the user has not pressed a key for a second (and a second after the start), and a cover that is asked for sooner is answered "not now" without opening the book in the list's frame, so the zip and XML of a book (some 20 KiB of memory) no longer come beside the list or an overlay the user works in. The screens ask again when it is quiet.
