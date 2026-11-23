### Fixed

- **The device no longer stays busy while a plugin waits for memory to
  start** (#483). When a sleeping plugin could not be started again at
  once (for example after waking up with a large book open), the reader
  kept working without a pause until it could, instead of waiting quietly
  for the next try. That cost battery.
