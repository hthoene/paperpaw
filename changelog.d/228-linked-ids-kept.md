### Changed

- **A built-in plugin's id takes no heap** (#248, D4). `Kernel::register_linked` takes the id as the firmware keeps it (`&'static str`), and its context, the owner directory and the linked set borrow that text instead of copying it, and a command or a callback's stage takes its owner id without a copy. The declaration store keeps an image-only owner's batch starts and kinds in one allocation. Loaded (card) plugins are unchanged.
