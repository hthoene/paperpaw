### Changed

- **Plain text is a native plugin only.** The old interface-6 form of the plain text format plugin is removed; the native plugin opens `.txt` files as before.
- **Android and web builds cannot open books for now.** They link the app without the native reader, and the interface-6 reader and plain-text format were the only ones they had (EPUB, FB2, HTML, Markdown and comics were already native only). Until those shells run the native reader (owner decision 185, temporary), these builds start without a reader. The X4 family and the desktop emulator are not affected.
