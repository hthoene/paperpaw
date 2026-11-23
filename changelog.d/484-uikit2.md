### Fixed

- **File sync with servers that answer with whole addresses** (#484). Where a WebDAV server (IIS, for example) lists a folder with whole addresses, the folder came back as a subfolder of itself and File sync and Picture frame went into `Photos/Photos/`; the folder is left out of its own listing again.
- **The clock in the status bar is no longer cut beside a long centre** (#484). With a long text in the middle of a status bar, the gap before it was taken out of the item on the left (`12:…`); it now goes between them, and a bar without a right part keeps no gap at its end.
- **Long screen titles wrap again before they are shortened** (#484). A title too long for the header at the small size is put on two lines (or three, a size smaller) where the bar holds them, as in Beta 3, before it ends in "…"; a long title is also sized with far fewer measurements.
- **A dashboard widget with odd targets no longer moves the focus wrongly** (#484). Up and down on a widget whose touch areas reach past the screen's largest coordinate went to the wrong target.
- **Note widgets keep the lines of a note** (#484). A note's line breaks were lost where the widget wraps its text to fit (the note ran on as one paragraph); each line of the note starts a line again.

### Changed

- **Moving the focus in the status items editor redraws two rows, not the screen** (#484); a button that moves nothing (at the end of the list, or a held item at the end of the shown ones) redraws nothing.
