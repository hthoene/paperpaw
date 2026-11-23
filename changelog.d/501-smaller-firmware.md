### Changed

- **The firmware takes about 26 KB less flash** (#501). The UI kit keeps
  every plugin's open screens and grows its lists with one code for all of
  them instead of a copy per screen type, draws a page's operations in one
  function, and the store sorts its installed plugins with the shared
  sorter. Nothing changes in what is shown or how fast.
