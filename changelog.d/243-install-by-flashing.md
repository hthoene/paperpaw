### Changed

- **The install image is checked against PaperPaw's own app partition, not CrossPoint's slot** (Decision 179). From Beta 4 PaperPaw installs by flashing; the native images are larger than CrossPoint's 6,553,600-byte app slots, so CI no longer fails a build for that (it says how far over the slot the image is) and fails one that no longer fits PaperPaw's own 8,323,072-byte partition. A device that keeps CrossPoint's layout still gets the update page's notice before a download that cannot fit.
