### Fixed

- **A long setting text is encoded for a preview** (#484): `settings::encode_value`
  refused any text over about 50 bytes with `NoSpace`, however large the caller's
  buffer; it now encodes every value that fits the buffer.
