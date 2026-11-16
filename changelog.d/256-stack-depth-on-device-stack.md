### Changed

- **The host tests now catch a route that goes too deep for the X4's stack** (#256). A new test crate (`tools/stack-budget`) runs the FAT calls of a plugin's files host, the bounded tree removal, the picture-folder provider on a FAT card and the files service's listing on a thread with the device's stack and measures how deep each goes; a change that deepens one of them past its ceiling fails the normal test run instead of showing as a boot loop on the device. No change for users.
