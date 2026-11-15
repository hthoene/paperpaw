### Changed
- The S3 frame scan can hold every function of a plugin family to the 2,048 B frame limit (`frames-in` line), not only the ones a root reaches; CI runs it as a second, report-only scan of the S3 image with the linked plugins' file. The update and reader plugins' handlers are no longer inlined into one 2 KB frame.
