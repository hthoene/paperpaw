### Changed

- **Seven kernel functions use less stack, for the x4-pro and x4-classic** (#263). The frames that
  call into a plugin, set the bars up, start the catalog's job, read a JSON file and convert an
  HTML file were over the 2,048 bytes a host frame may have on those devices; each is now under
  it, with nothing changed in what they do. A JSON file is read 1,536 bytes at a time instead
  of 2,048.
