### Fixed

- A picture block whose picture is missing or fails the image check no longer closes the book:
  the block keeps its alt text and the host notes it once a document, as `document.md` section 5
  says (an EPUB failed to open after three such blocks) (#243).
