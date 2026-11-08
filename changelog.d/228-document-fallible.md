### Fixed

- **Opening and reading HTML, Markdown and EPUB no longer aborts on a full heap** (#228). The shared reading code behind the format plugins answers a refused allocation with an out-of-memory refusal instead of aborting; successful reads are unchanged. A sweep test refuses each allocation in turn.
