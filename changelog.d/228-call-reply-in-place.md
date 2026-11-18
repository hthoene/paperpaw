### Changed

- A native plugin's service call no longer needs a second buffer on the host: the reply is built
  straight in the plugin's output buffer (up to 4 KiB less heap at the peak of every call on the
  X4). After a service's own failure the reply room of that buffer is cleared; a refused call still
  leaves it untouched, and an input that overlaps the output is refused (`ERR_INVALID`).
