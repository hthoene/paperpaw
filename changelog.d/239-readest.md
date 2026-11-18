### Changed

- Progress sync (kosync) now also continues a comic (CBZ) in Readest and back: a book of whole pages sends its 1-based page number as the progress, as KOReader and Readest do for a fixed layout, and a page number received opens exactly that page. Books are matched with Readest by the same partial MD5 and EPUB positions by the same XPointers, so those already worked (#239).
