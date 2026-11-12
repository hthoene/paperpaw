### Fixed

- **A picture asked for larger than 1,600 pixels is made at 1,600** (#252). `core.document` op 16 (`doc_image_ask`) clamps a `side` above 1,600 instead of refusing it, as `document.md` section 5 and `image_prepare` already do.
