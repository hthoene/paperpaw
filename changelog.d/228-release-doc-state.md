### Changed

- **Document rooms given back after the details run; `core.work` made at its first call** (#228,
  #248). When the host's details run ends and no document is open, the document table's room,
  the reader's books and the places of the format calls are freed; the next open reserves them
  again, fallibly (an open without room is `ERR_NO_MEMORY` and the job waits). The work
  scheduler and its view are made at the first `core.work` call, in the host's fallible shared
  pointer.
