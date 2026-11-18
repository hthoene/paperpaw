### Changed

- **The EPUB, HTML, FictionBook, Markdown and comic book plugins are five crates now** (#228). Their shared conversion lives in `paperpaw-epub-family`, and each plugin (`format-epub`, `format-html`, `format-fb2`, `format-markdown`, `format-cbz`) has its own interface-7 entry, ready to be packed as a native plugin file. Nothing changes for readers yet: the interface-6 plugins stay registered until the per-format removals.
