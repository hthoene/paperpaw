### Fixed

- **Formulas in books** (#484): a formula with an element left open inside it
  no longer swallows the rest of its chapter, and a formula nested very deeply
  no longer runs out of memory.
- **Books after a power cut** (#484): a chapter, a book's contents and long
  cache files are no longer kept cut short when the power goes while a book is
  converted; the part cut off is converted again.
