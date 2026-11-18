### Added

- **Page replies of a format provider in the native SDK** (#228). `FormatCall::rows`
  writes the sections, table-of-contents, links and fonts pages of `core.format` row
  by row into the caller's output (`FormatRows`, `PageEnd`, `LinkKind`), always
  leaving room for the closing record, and `FormatCall::reply_image` answers cover
  and image requests. Nothing allocates.
