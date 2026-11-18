### Changed

- **A screen that could not be opened says so** (#228). When a native plugin's screen cannot be made (the plugin stopped, an argument was refused, …), the user sees the notice "This screen could not be opened." besides the diagnostic; the words never name the screen or its parameters, and after a lack of memory it is skipped (the log still says it), since the notice screen would meet the same trouble.
