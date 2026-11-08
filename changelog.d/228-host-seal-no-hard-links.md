### Changed

- **Saved passwords on a folder with no hard links** (#228). The desktop and Android key file (`paperpaw-host-seal`) is put in place by a hard link so that an existing key is never replaced; where the folder has none (a FAT folder), the same comes from an exclusive lock file around the check and a rename, so a key can still be made there. A lock left by a creator that was cut off is taken away after ten seconds.
