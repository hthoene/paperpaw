### Fixed

- **A document cache that is removed half way no longer looks whole** (#228, `docs/abi/document.md` 2.1). Removing a conversion's cache folder takes its `.complete` stamp away first, so a removal that is cut or fails (or one that goes on in the next call) can never leave a folder whose stamp says it is finished with its pages gone.
