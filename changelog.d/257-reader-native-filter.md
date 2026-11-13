### Added

- **Reader: the reading aids' rules in the native reader** (#257). The native reader asks the `reader.filter` stage when a book opens and again when another plugin's setting changes, checks each answer once and applies bold word beginnings, replaced words and the pictures rule (shown, captioned or left out) to each block before it is laid out, as Beta 3 did. Pages laid out under other rules are counted anew. It keeps only the answers' bytes (at most 3,840 each) while a book is open. Devices that run no stages yet (the X4 until its stack gate) read the book unfiltered.
