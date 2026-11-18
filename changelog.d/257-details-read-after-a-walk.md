### Fixed

- After the library looked for books, the titles and authors of EPUB, DOCX and MOBI books are read right away on an idle device. Before, reading began only with the next key press or, on the X4, with the next battery reading up to a minute later, and not at all if the device went to sleep first (#257).
- While the titles and authors of books are read in the background, the device rests between the steps again. Before, it could stay busy without a pause for as long as a book was being prepared, which cost battery (#257).
