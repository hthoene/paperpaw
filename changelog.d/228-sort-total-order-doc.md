### Fixed

- **The shared sorter no longer panics on a comparison that is no total order** (#228). Lists of 13 to 2,048 items were ordered through their positions by the standard library's unstable sort, which may panic on a comparison that contradicts itself (a plugin can hand `stable_sort_by` one); an in-place heapsort now does it, with the same result for any valid comparison, no extra memory and about 1.2 to 3 times the time of the standard sort on 2,000 entries. Every path gives some order of the same items for such a comparison, and tests cover them all.
