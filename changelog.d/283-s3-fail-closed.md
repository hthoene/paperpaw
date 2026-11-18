### Changed

- **The S3 stack scan no longer misses a jump table silently** (#283). A function with a `jx` (a switch) now needs a jump table that one of its own `l32r` literals gives, of any length; a function where the scan cannot tie a table that way fails unless the budget names it with `allow jx <function> <why>` after someone has read it. Build tooling only; interface 7 is unchanged.
