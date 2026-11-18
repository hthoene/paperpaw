### Changed

- **A book format may sleep** (#248, D2). Formats are no longer kept awake by the kernel's park checks: a format the shell flags to sleep is parked after its start and made at the next safe point once a book needs it. The document service keeps such an open preparing instead of answering busy (Host, #2717). No format is flagged yet.
