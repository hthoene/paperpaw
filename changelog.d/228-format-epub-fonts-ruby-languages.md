### Added

- **format-epub sends embedded fonts, ruby kinds and inline languages through `core.format@1`** (#228). Embedded faces are copied in chunks to the document cache (`fonts/<i>`, through a temporary name) and listed with their size; ruby, phonetic and gloss notes keep their kind; a block's inline languages go out as `fmt.inline_lang` rows that its runs name by index. Sniff tests cover a wrong-case, a deflated, a not-first and a too-short `mimetype` head. Nothing is active until the host calls providers.
