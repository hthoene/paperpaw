### Changed

- **The library's move job runs in the kernel's main loop** (#228). A move of the
  user's files (root 1) is carried by one bounded step a pass of the service pump,
  outside callbacks and with the kernel's own catalog on the same card; the
  library's book-moved events and the settings that follow run right after the
  pump. A step the card or memory refused waits two seconds and the job is kept;
  an interrupted job is finished at the first pass after a start.
