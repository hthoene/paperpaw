### Changed

- **Statistics screens on interface 7: no whole list of books in memory** (#228). The books, book, overview and calendar screens now read `books.ini` one book at a time instead of loading it whole, so opening them takes the same memory for a library of ten thousand books as for ten. The books screen still lists the 200 books read last; the calendar looks up the titles of the day's books as the file is read.
