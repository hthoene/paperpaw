### Changed

- **Catalog texts take small buffers first** (#248). `Messages` started with a 4 KiB message
  buffer and a 1 KiB argument buffer; they now start at 512 and 256 bytes and grow (once, to what
  the host says a longer text needs) only when a text is longer. Most screens hold about 4 KiB
  less while they make their texts.
