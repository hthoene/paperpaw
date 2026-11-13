### Fixed

- **Native status lines show again where the kernel's battery is listed** (#243, #228). The registry's status listing (op 21) marked the kernel's battery items as drawn by their owner, a mark the contract keeps for whole status lines (`registry.md` section 3). The SDK refuses such a page, so every status line that reads its items from the registry (the reader's line, home's top row, picture-home's bar) and the item choosers failed and stayed empty. The kernel's items are now listed as any item; that the battery is drawn is still said by its value (`core.ui` op 14), as before.
