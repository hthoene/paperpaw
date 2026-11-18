### Fixed

- **The native sleep screen falls back to the logo or the cat when its widgets fail** (#228, #244). As in Beta 3, the message shows with the logo above it when the widgets chosen come up empty or cannot be laid out, and the cat shows when the widget over the page cannot be made and there is no cover. The widgets are made at the first draw, which can ask for no picture, so the native screen now asks for that picture when it opens.
