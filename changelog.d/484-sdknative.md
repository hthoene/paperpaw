### Fixed

- **A long setting text is encoded for a preview** (#484): `settings::encode_value`
  refused any text over about 50 bytes with `NoSpace`, however large the caller's
  buffer; it now encodes every value that fits the buffer.
- **A screen's last word on the row of button hints counts** (#484):
  `ScreenDecl::hints_hidden()` followed by `.hints(..)` (or the other way round)
  declared both, which the host refuses; the later call now replaces the earlier.
