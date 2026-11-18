### Changed

- **Notes vault: Markdown notes are a format for interface 7** (#228). Opening a note takes no more
  memory than the device has: a note read on a full heap is answered "out of memory" and tried again
  later, never stops the device. The notes' formats (front matter, wiki links, embeds, tables, callouts)
  read as before.
