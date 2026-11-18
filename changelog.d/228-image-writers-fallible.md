### Fixed

- **Decoding with transparency or colour, and screenshots, no longer abort on a full heap**
  (#228). The alpha and colour picture writers write their headers from the stack, and the
  PNG writer takes one fallible reservation per row (and one for its header) instead of
  growing buffers infallibly.
