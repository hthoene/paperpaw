### Changed

- **No kernel *Open with…* in the book menu with the linked native reader** (#228, #257). With `native-linked-reading` the kernel no longer registers the book menu's *Open with…*: its resolver is interface 6's, which interface 7 does not carry (`docs/abi/document.md` section 1). The book menu has no *Open with…* in that build until a native one exists. The file manager's one-time *Open with…* stays, and builds with interface 6's readers keep the entry.
