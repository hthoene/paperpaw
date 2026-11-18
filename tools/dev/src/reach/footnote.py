#!/usr/bin/env python3
"""Writes footnote.epub: a book with a footnote on its first page, for the
declared way into the reader's footnote (ways.rs). Run it here; the file
it writes is the same every time."""
import zipfile

FILES = [
    ("mimetype", "application/epub+zip"),
    (
        "META-INF/container.xml",
        """<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
<rootfiles><rootfile full-path="content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>
""",
    ),
    (
        "content.opf",
        """<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="id">
<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
<dc:title>A Footnote</dc:title>
<dc:creator>PaperPaw</dc:creator>
<dc:language>en</dc:language>
<dc:identifier id="id">paperpaw-dev-reach-footnote</dc:identifier>
</metadata>
<manifest>
<item id="text" href="text.xhtml" media-type="application/xhtml+xml"/>
<item id="notes" href="notes.xhtml" media-type="application/xhtml+xml"/>
<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
</manifest>
<spine toc="ncx"><itemref idref="text"/><itemref idref="notes"/></spine>
</package>
""",
    ),
    (
        "toc.ncx",
        """<?xml version="1.0" encoding="utf-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
<head><meta name="dtb:uid" content="paperpaw-dev-reach-footnote"/></head>
<docTitle><text>A Footnote</text></docTitle>
<navMap>
<navPoint id="text" playOrder="1"><navLabel><text>The page</text></navLabel><content src="text.xhtml"/></navPoint>
<navPoint id="notes" playOrder="2"><navLabel><text>Notes</text></navLabel><content src="notes.xhtml"/></navPoint>
</navMap>
</ncx>
""",
    ),
    (
        "text.xhtml",
        """<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>The page</title></head>
<body>
<h1>The page</h1>
<p>A page with a note at its foot<a epub:type="noteref" href="notes.xhtml#n1">[1]</a>, for the check that the buttons lead to it.</p>
</body>
</html>
""",
    ),
    (
        "notes.xhtml",
        """<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Notes</title></head>
<body>
<h1>Notes</h1>
<p id="n1">[1] The note at the foot of the page: what the reader shows over it.</p>
</body>
</html>
""",
    ),
]

with zipfile.ZipFile("footnote.epub", "w") as book:
    for name, text in FILES:
        info = zipfile.ZipInfo(name, date_time=(2026, 1, 1, 0, 0, 0))
        info.compress_type = zipfile.ZIP_STORED
        info.external_attr = 0o644 << 16
        book.writestr(info, text)
