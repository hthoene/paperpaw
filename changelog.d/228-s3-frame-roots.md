### Changed

- The S3 frame scan (`paperpaw-sign frames-s3`) now also follows the code behind trait objects:
  `root trait <path>` and `root impl <type>` budget lines root every impl method of a trait or a type
  (the card's `Storage`, the font packs, `Display`/`Write`, the allocator), so a frame of 2,048 B or
  more, or a call it cannot follow, in `FatStorage`, `Metered`, `Recalled` and the rest is found.
  An `allow form` line names the special register it writes (`wsr.scompare1`); a write of `LITBASE`
  is refused. Both budgets (`frame-budget-s3.txt`, `-classic.txt`) cover the larger set, each line
  with its reason.
