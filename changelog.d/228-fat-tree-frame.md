### Fixed

- **The folder-tree removal of the FAT storage takes less stack** (#228). On the
  X4 Pro (ESP32-S3) `remove_tree_step` needed 2,928 bytes of stack, over the
  2,048 bytes anything a plugin can reach may take. The walk now keeps each
  step in its own function and changes its folder frames in place instead of
  passing them around; the largest frame of the scan is now 1,712 bytes. What
  the removal does, its limits and its behaviour at a power cut are unchanged,
  and it takes no more heap.
