### Changed

- **Plain text is an interface-7 plugin** (#228). `format-txt` declares its format (`txt`, `text`)
  and answers the host's `core.format` calls from a native entry, with the same blocks as before.
  `paperpaw_sdk::native::reply::Reply::write_with` is public so a provider can encode its answer
  straight into the host's reply buffer.
