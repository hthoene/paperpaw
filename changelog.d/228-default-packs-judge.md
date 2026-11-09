### Fixed

- **A broken `plugin.ini` no longer keeps English hyphenation away for good** (#228, follow-up of #737): the default pack is kept, and marked as dealt with, only where the plugin found on the card is one the reader can use (it names itself `hyphenation`, is made for formats 5 or later and names a table that is in its folder). A `plugin.ini` that is no such plugin is written over by the pack, and a file that cannot be judged is left as it is and looked at again at the next start. The build of the package now runs the package check, so a bad embedded pack fails the build.
