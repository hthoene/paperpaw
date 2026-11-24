### Fixed

- **Language packs: the setup step *Add more* explains itself again in every language, not only in English and German** (#474). The packs carry its help under the key the store reads since #472 (`store.setup.help`); every UI language pack has a new patch version. A new test checks that every pack holds only keys a plugin or the kernel declares.
