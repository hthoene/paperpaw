### Changed

- `command.set_hint` labels are charged to the submitting plugin (`action +
  words + 4` bytes a label, summed over its open screens) against a budget of
  4,704 bytes on the small-heap class and 18,816 bytes elsewhere. A label over
  it is `ERR_NO_MEMORY` and the earlier labels stay. The charge is released
  when a label is replaced or cleared, when its screen closes and when the
  plugin stops. The budget is read in one place, to switch to the plugin's own
  heap budget once the frame-owner charging of `abi/heap-word.md` exists in core.
