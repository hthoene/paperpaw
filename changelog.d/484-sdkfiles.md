### Fixed

- Highlights and bookmarks with a long note or text (over 4 KB) are no longer cut, and so lost, when another annotation of the same book is added, changed or removed: such a change is refused and the file stays as it was (#484).
- An annotations file saved by an editor with a byte order mark or Windows line breaks reads as one without them: its version is not misread after a change, and notes no longer end in a stray character (#484).
- Annotations kept before conversions were (older PaperPaw) stay marked as such when their file is upgraded, so they are never moved to the wrong place when a book is opened with other options (#484).
- Where the device left a book is read even when the half of its position file being written was cut off (#484).
