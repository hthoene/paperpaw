### Fixed
- The bounded removal of a tree (`remove_tree_step`) no longer stops with `NotFound`, leaving files behind, on a storage whose folders are only their files (the in-memory store of the host and of tests): a folder whose last file was just removed counts as empty and the walk goes on in its parent.
