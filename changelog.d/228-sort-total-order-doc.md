### Documentation

- **The shared sorter says what a comparison that is no total order does** (#228). It gives an unspecified order and never loses an item; lists of 13 to 2,048 items go through the standard library's unstable sort, which may panic on it, so a comparison must be a total order (also noted on `stable_sort_by` of the UI kit). A test covers the paths that do not panic.
