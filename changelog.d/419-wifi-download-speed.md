### Changed

- **Much faster Wi-Fi downloads on the X4 and X3** (#419). Updates, plugins and
  language packs download over Wi-Fi about twice as fast: the reader keeps
  receiving while it writes to the card, writes the card in larger aligned
  pieces, and secure connections use the cipher (ChaCha20-Poly1305) that the
  small processor decrypts about ten times faster than AES-GCM, where the
  server offers it.
