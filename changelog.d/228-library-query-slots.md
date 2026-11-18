### Changed
- Library query: each book a query keeps costs one allocation and 32 bytes less (sort text and path in one buffer, 32-bit line offset), and the recently-opened list is reserved for the books it names (#228).
