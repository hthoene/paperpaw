### Changed
- Opening and reading books needs 4 KiB less memory: a plugin's file read goes straight into its own buffer instead of through a copy (#248).
