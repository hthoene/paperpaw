//! Making the sorted file, as the kernel does: a few steps of reading
//! the library's books into runs sorted in memory, then a few steps of
//! merging the runs into the file, written beside it and put in place
//! whole. It holds a run and the series counted (some 24 KB on a device
//! of little memory) whatever the library's size; paused, it writes the
//! run it holds (a shorter one) or lets go of what it read ahead of the
//! runs, and keeps the series counted.
