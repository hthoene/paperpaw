### Fixed

- **Input filters: bad answers about a held key or finger are counted** (#228).
  When a filter answered with an unknown verdict or an invalid replacement
  for the rest of a key or finger another filter had taken, the answer was
  dropped without a trace; it is now counted and logged like a bad answer to
  the first press, and the input is still taken.
