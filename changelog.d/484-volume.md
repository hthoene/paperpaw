### Fixed

- **A folder that grows on a FAT card no longer shows old data after a power cut** (#484).
  When a folder needed another cluster, the cluster was linked to the folder before it was
  cleared; losing power between the two left what a deleted file had there in the folder,
  read as entries that named other files' clusters (removing the folder then freed them).
  The cluster is now cleared before it is linked, and one taken but not linked yet is freed
  at the next start.
- **A damaged FAT card can no longer have a file's data written over its FAT** (#484). A
  file or a link in the FAT that named a cluster the card does not have (1, or one past its
  end) was read and written at a place computed from it, which could be the FAT itself. It
  now reads as a damaged card, and nothing is written.
- **A FAT folder whose clusters run in a loop no longer hangs the reader** (#484). Listing
  such a damaged folder went on for ever; it now stops with an error after the 65,536
  entries a FAT folder can have, and a folder no longer grows past them.
- **Removing a damaged file no longer marks the FAT card as not cleanly removed** (#484).
  A file entry that named cluster 1 cleared the card's clean-shutdown mark in the FAT when it
  was removed, so computers asked to check the card. Only the card's own clusters are freed
  now.
- **A FAT card whose boot sector does not add up is refused** (#484). Numbers in the boot
  sector that overflowed, or a FAT too small for the card's clusters, could place the FAT
  over the data or make looking for free space fail; such a card now reads as damaged.
