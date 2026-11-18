### Added

- **The block stream of a format provider in the native SDK** (#228).
  `FormatCall::reply_block` writes a document-model `Block` as the record list of
  `core.format` op 8 (`BlockReply`): header, text parts, runs, offset maps, picture,
  table columns and cells, annotations and inline languages, in fragments that end
  between records with the block's generation. Nothing allocates.
