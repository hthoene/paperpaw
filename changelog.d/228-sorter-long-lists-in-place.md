### Changed

- **Long lists are sorted in place, and the ui-kit sort is the shared one** (#228). The shared sorter held `4 * n` bytes for the positions of a list (40 KB for 10,000 entries, three times that with cached keys); from 2,048 entries on it sorts in place and allocates nothing (about 2.5 to 3 times slower there), so a long list never needs a large block on a full heap. `paperpaw_ui_kit::mem::stable_sort_by` is no longer a quadratic insertion sort but the shared sort, with the same order. The Bluetooth device list places each device found instead of sorting the whole list again.
