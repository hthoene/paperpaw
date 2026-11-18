### Changed

- **One shared sorter instead of dozens of copies** (#228). Sorting by title, key or rank no longer compiles a separate copy of the standard sort for every list type. The new `paperpaw-sort` crate (also `paperpaw_sdk::sort`) is stable and never panics; short lists (up to 12 items) are sorted in place without any memory, and longer ones fall back to an in-place merge when memory is short, so a sort never fails on a full heap. The X4 image shrinks by about 127 KB; sort order is unchanged.
