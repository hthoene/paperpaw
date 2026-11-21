### Fixed

- **Stylesheets of one's own did nothing to EPUB books** (#459). A `styles.css` in a book's folder, and the one named like the book (`Alice.css` for `Alice.epub`), apply after the book's own styles again, as in Beta 3 and as the FB2, HTML and Markdown formats already did (EPUB 1.0.7). A book converts once more after the update.
- **A link to a heading of another note opened the note at its top** (#459). `[[Note#Heading]]` in a notes vault opens that note at the heading again, as in Beta 3; *Back* returns to the note it was followed from (Notes vault 1.0.5, Reader 1.2.3). Notes convert once more after the update.
