### Fixed

- **Opening and reading HTML and Markdown (the shared reading code EPUB conversion also uses) no longer aborts on a full heap** (#228). The shared reading code answers a refused allocation with an out-of-memory refusal instead of aborting; successful reads are unchanged. A sweep test refuses each allocation in turn.
