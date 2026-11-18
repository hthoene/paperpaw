### Added

- `paperpaw-sign frames-s3 <elf> --budget <file>` reads the call graph of an S3 firmware from the budget's roots and function-pointer tables, holds every function it reaches to a frame under 2,048 B, and refuses `call0`/`call4`/`callx0`/`callx4` and any write to `a1` other than `entry`. Calls it cannot follow (through a register, into ROM or a blob, to code with no symbol or size) fail unless an `allow` line names them, and an `allow` line that matches nothing fails.
