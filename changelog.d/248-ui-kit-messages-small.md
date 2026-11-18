### Changed

- **Catalog texts take small buffers first** (#248). `Messages` started with a 4 KiB message
  buffer and a 1 KiB argument buffer; they now start at 512 and 256 bytes and grow only for a
  longer text: a copied text is asked again once with 4 KiB, a formatted text or digits once with
  the length the host reports, arguments once with 1 KiB. Most screens hold about 4 KiB less
  while they make their texts.
