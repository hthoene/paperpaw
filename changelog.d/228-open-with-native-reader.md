### Changed

- **No kernel *Open with…* in the book menu with the linked native reader** (#228, #257). With `native-linked-reading` the kernel no longer registers the book menu's *Open with…*: its resolver is interface 6's, which interface 7 does not carry. The file manager's one-time *Open with…* stays. Builds with interface 6's readers keep the entry.
