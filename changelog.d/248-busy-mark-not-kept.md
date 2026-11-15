### Fixed

- **The busy mark cannot stay on the X4's screen when memory runs short** (#248). After slow work the panel's two picture memories are filled again with what was shown; where the little memory that takes (9.6 KB) is refused, the shell now shows the next frame whole (as after a paced frame) instead of only logging that the mark was not kept, which could leave a ghost of the mark at the next quick refresh.
