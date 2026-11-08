### Changed

- **News on interface 7, second slice** (#228). A feed (RSS or Atom) is made into a page to read
  by a converter that reads the XML with the SDK's fallible reader and keeps only one article in
  memory at a time, so a refused allocation ends the fetch with an error and the old book stays,
  instead of aborting. The page is the same, byte for byte, as before. The native plugin takes over
  once the host runs native plugins.
