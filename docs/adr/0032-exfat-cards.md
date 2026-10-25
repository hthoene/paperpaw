# 0032. exFAT cards

- Status: Accepted
- Date: 2026-10-25

## Context

The X4 read only FAT. Most microSD cards of 64 GB and more are sold
formatted as exFAT, and CrossPoint, the X4's other firmware, reads exFAT,
so readers coming from it often bring such a card. Up to beta.8 the X4
mounted it as no storage at all: nothing was kept, the first-run setup
came back at every start (a reader on Reddit), and since beta.8 a notice
asks to format the card as FAT32, which a reader may not know how to do
and which erases the books on it.

What the X4 keeps on the card (books, reading progress, notes,
statistics) must survive an empty battery at any moment, as it does on
FAT, where every write goes through a temporary file and a move is
journaled. And the X4 has about 100 KB of heap free: the allocation
bitmap of a 64 GB card in clusters of 32 KiB alone is 256 KiB.

## Decision

The SD card's file system is FAT or exFAT, told apart by the volume's
first block (`BlockDisk::find`), on the same block cache that writes
changed blocks back in the order they were changed. `ExFatStorage`
(crate `paperpaw-fat`) is PaperPaw's storage on exFAT; nothing is ever
formatted.

- **The driver** is hadris-fat's exFAT driver (MIT), after a look at
  every exFAT crate on crates.io: the only one that is `no_std` with a
  blocking API, writes as well as reads (folders that grow, long names
  through the up-case table, chains and contiguous files, rename,
  truncate), streams the bitmap and the FAT through one block (about
  8 KiB of state), orders its writes for crashes (clusters taken before
  an entry set names them, freed after it is gone, `VolumeDirty` set
  before the first write and cleared on sync) and is tested against
  `fsck.exfat`. It is pinned to one version and patched where PaperPaw
  needs it (`patches/hadris-fs`, `patches/hadris-fat`, "Modified for
  PaperPaw"): built where pointers have no atomics (the ESP32-C3), an
  entry set taken away without freeing its clusters (`unlink_entry`),
  what a cut at an earlier mount left in a folder settled (`sweep`), and
  a smaller buffer of zeros in flash.
- **Writes as on FAT**: a file is written whole under `<name>~pp.tmp`,
  synced, renamed to `<name>~pp.new`, then renamed over the file. The
  names of the moment hold the file's name, so that a replacement cut off
  in an entry set that crosses two blocks still knows which file it was
  for. A move is first recorded in `~PPMOVE.BIN` at the root.
- **After a cut**, when a folder is first looked at: entry sets cut off
  between two blocks are sealed again as they stand or let go
  (`sweep`); two names that hold the same clusters (a rename cut off
  after its new entry set) lose the stale one without freeing them; a
  whole new content takes its file's place, a part of one goes; the
  journal's move is finished. What may stay is space: clusters taken by
  no file, or linked past a file's length before its new length was
  written. Nothing reads them, and a computer's checker gives them back.
  The dirty flag stays set after a cut, as the specification asks.
- **The notice** of beta.8 stays for NTFS, cards without a file system
  PaperPaw reads, unreadable and missing cards.

## Consequences

- exFAT cards work as they come, books on them kept. Tests run on
  images made by `mkfs.exfat` and filled by exfat-fuse (another
  implementation), check every image written with hadris-fat's checker
  and, where installed, `fsck.exfat`, follow a model in random
  operations, and cut off writes, new files, moves, removals and appends
  after every block, at every place of an entry set in a block.
- The X4's firmware grows by about 105 KB of flash (619 KB stay free in
  the smallest update slot); the heap on an exFAT card is about 8 KiB
  above FAT's (the emulator, which mounts card images with `--card`,
  measures it on the X4's profile).
- The first write that takes clusters after a mount counts the free
  clusters once, a block of the bitmap at a time (on a 64 GB card 512
  blocks); free space is known from then on.
- A newer hadris-fat is taken only after its changes are read and the
  patches carried over; the tests above decide.
