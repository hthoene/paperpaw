### Changed

- **A screen that could not be opened says so** (#228). When a native plugin's screen cannot be made (no memory, the plugin stopped), the user sees the notice "This screen could not be opened." besides the diagnostic; the words never name the screen or its parameters, and under memory trouble the notice is skipped rather than allocating without a check.
