### Fixed

- Pictures that a format has not decoded yet (DOCX, MOBI) are no longer left out: the host passes
  them on without an id, at the size they will have, and the reader asks for them by index; only
  their path and a size of 1 to 1,600 pixels are checked until then (#243).
