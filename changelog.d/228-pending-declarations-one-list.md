### Changed

- **A native plugin's pending declarations cost a flat amount** (#228). The
  host keeps one sorted list per plugin instead of a table per declaration
  kind, so the memory each registered plugin holds no longer grows with every
  new kind. Admission and limits are unchanged.
