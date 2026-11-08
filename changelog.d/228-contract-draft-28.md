### Changed

- **Interface 7 draft 28: the contract for images, hyphenation, covers, icons,
  setup steps, book badges and lookups that fetch** (#228). Plugins can read
  picture sizes and draw pictures, ask for hyphenation points, cover text and
  named icons through new host slots; setup steps and book badges have their
  host records; a lookup provider that must go online answers "pending" and
  tells the reader when its answer is ready instead of blocking it. Staged
  downloads now use `.pp-staged` names, so a user's own `.part` file is never
  overwritten. `EventKind::Answered` is gone: answers to a plugin's own
  questions arrive without subscribing.
- Interface 7 host slots 36 `image.info` and 37 `canvas.draw_image`
  (`docs/abi/document.md` section 5) and `doc_open` root 5, a file handed over by
  the plugin that opened the reader (section 2.1). Hosts answer `ERR_UNSUPPORTED`
  until wired (#228).
- Interface 7 host slot 38 `text.hyphenate` (`docs/abi/document.md` section 6.1):
  the host hyphenates a word from the card tables of `[hyphenation]` data
  plugins; the reader holds no table (#228).
