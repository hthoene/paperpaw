### Changed

- **Opening a settings group asks the registry for far fewer pages** (#243). The native settings page asked for the whole list of settings twice for a group (once for its rows, once to see which parts hold something), and a third time to find the group of a setting to focus. It now asks for the group's own settings, and for each part only until one is found; a setting to focus or to choose from is one call. With 426 settings on a real device this is about 3 pages instead of 16 to 24; the screens are the same.
