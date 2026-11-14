### Fixed

- **Leaving the file manager no longer queues a redraw of the screen it closes.** *Back* in the top folder redrew the folder it was about to close, and the kernel dropped that redraw with a warning ("its screen is covered"). The browser now draws anew only when going up a folder changed what it shows.
