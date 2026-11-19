### Fixed

- **A book opened with a dialog over it no longer fails its draws** (#359). When another screen (a dialog) opens over the reading screen in the round the book opens, the host draws the page before it tells the reader it is shown, and the reader's draw ended with `ERR_STATE` ("call 10 ended with -6") until the dialog closed. The reader now draws its newest screen then.
