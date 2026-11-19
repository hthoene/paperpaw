### Changed

- **Fewer host calls for each page turn** (#393). The reader now takes a
  reading font from the device only when a screen first draws with it,
  not all four at each band of the page. A page turn makes about 45 fewer
  calls (475 instead of 517 on the sample page).
