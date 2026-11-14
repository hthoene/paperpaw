### Changed

- **The firmware no longer carries the names of all icons** (#258): 58 KB less. The SDK finds an
  icon by the hash of its name. `icons::char` answers as before for every icon's name; a name that
  is no icon's is taken for one where its hash happens to be an icon's (about one in a million).
  The names stay in the SDK for tools and tests (`icons::names()`); nothing in the firmware calls
  it.
