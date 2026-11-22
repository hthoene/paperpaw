### Fixed

- **A folder that grows on a FAT card no longer shows old data after a power cut** (#484).
  When a folder needed another cluster, the cluster was linked to the folder before it was
  cleared; losing power between the two left what a deleted file had there in the folder,
  read as entries that named other files' clusters (removing the folder then freed them).
  The cluster is now cleared before it is linked, and one taken but not linked yet is freed
  at the next start.
