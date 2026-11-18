### Fixed

- **A memory card can be swapped as often as you like while PaperPaw runs** (#151). Each new mount of a card taken out and put back kept about 13 KB of the old one's memory, so PaperPaw mounted a swapped card only once per start and asked for a restart after a second swap. The card's storage now owns its file system and gives everything of the old mount back (its open files, folders and caches), so every swap, also one while the device sleeps, is mounted anew with the memory of the first mount. What PaperPaw remembered of the old card's books is forgotten as well.
