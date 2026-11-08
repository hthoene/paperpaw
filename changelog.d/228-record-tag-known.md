### Added

- **Record tags can be checked without decoding** (#228). The generated
  records codec gains `record_tag_known(tag)`, so a plugin or the host that
  decodes records by type keeps the statuses of the `Record` enum path (an
  unknown tag is `ERR_UNSUPPORTED`, a known but unexpected one
  `ERR_INVALID`) without linking every decoder.
