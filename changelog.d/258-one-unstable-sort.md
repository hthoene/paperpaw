### Changed

- Lists that are sorted in place (book keys, a ZIP's index, declarations, the status line) share
  one compiled sort instead of a copy of the standard library's per element type: less flash
  on the X4 (#258). `paperpaw_sort` (and `paperpaw_sdk::sort`) gains `sort_unstable`,
  `sort_unstable_by` and `sort_unstable_by_key`: in place, no memory at any length, equal items
  may change places. They take several times as long as the standard library's on long lists
  of numbers (10,000 keys: under a millisecond on a PC), so measure where one is sorted often.
