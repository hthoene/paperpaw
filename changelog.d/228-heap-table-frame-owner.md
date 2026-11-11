### Changed

- **The host side of the plugin heap word, first part** (#228). The host has a `PpHeap` table (`handed_heap::<S>()`) over the same allocator as the heap slots, and every entry from the host into a plugin's code (its factory, destructor and callbacks) runs it as the "frame owner" and gives the previous owner back on every way out. Nothing charges a plugin's allocations yet and no shell passes the table to a loader.
