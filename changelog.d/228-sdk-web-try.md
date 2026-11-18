### Added

- **Fallible web helpers in the SDK** (#228). `web::try_clean_name`, `try_file_name`,
  `try_percent_decode` and `try_base_url` do what `clean_name`, `file_name`, `percent_decode` and
  `base_url` do, and return an error where memory is refused instead of aborting, so native
  plugins can name files and addresses on the small heap. The old functions are thin wrappers and
  give what they gave.
