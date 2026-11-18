### Added

- **XML: entity decoding that reports low memory** (#228). `paperpaw_xml::try_decode` and
  `Attrs::try_get` decode character and entity references with one fallible reservation of at
  most the raw text's length, so a feed full of entities (OPDS) gets an error on a full heap
  instead of an abort. `decode` and `Attrs::get` keep their behaviour.
