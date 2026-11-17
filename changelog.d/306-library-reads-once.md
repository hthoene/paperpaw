### Changed

- **The library answers faster** (#306). A library call reads the log of
  status changes once instead of once per scan and per row, and the
  number of books is counted once for a state of the catalog instead of
  on every call, so Home and the Library open sooner.
