### Added

- Native plugins: `command.redraw_area` (SDK `CommandSender::queue_redraw_area`) draws again only a rectangle of the plugin's screen, as interface 6's `Cx::request_redraw_area`. Each rectangle is drawn and weighed on its own, so pictures filled in after a page's text refresh as small areas and do not count as a page turn towards the periodic full refresh. Rectangles asked for together with a whole redraw are drawn in the frame after it, so a reader shows a page's text first and its pictures right after (#252).
