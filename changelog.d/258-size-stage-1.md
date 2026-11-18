### Changed

- **The firmware no longer carries the names of all icons** (#258): 58 KB less. The SDK finds an
  icon by the hash of its name. `icons::char` answers as before for every icon's name; a name that
  is no icon's is taken for one where its hash happens to be an icon's (about one in a million).
  The names stay in the SDK for tools and tests (`icons::names()`); nothing in the firmware calls
  it.
- **The firmware no longer carries the standard library's reader and writer of decimal numbers**
  (#258): about 36 KB less. The SDK writes an `f32` with the digits `Display` writes and reads a
  text of up to 19 digits to the float the standard library reads (of a longer one, its first 19).
  JSON numbers are read to the same `f64` as before. A new check (`tools/firmware-symbols.sh`) fails
  the firmware build when that code, or the icons' names, come back.
