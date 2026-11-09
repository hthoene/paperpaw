### Fixed

- **A document image from a format's `fmt.image` record** now carries "not decoded yet" as `doc_image.flags` bit 1 (#228); bit 0 there is colour, as in `image.info`: the cache file's header says colour and the screen keeps colour (never set on a gray screen).
