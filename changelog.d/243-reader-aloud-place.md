### Fixed

- **Reading aloud speaks in the native reader** (#243, #2426). The reader knew the page on show by the place it was asked for, the sentences by where the page's layout starts; where they differ (a text file whose book starts after a header, a page after a section's end) reading aloud said it failed and spoke nothing. The page on show now goes by its layout's start when reading aloud starts and after each sentence.
- **Read aloud right after the start** (#243). *Read aloud* pressed while the book read last is still opening is kept and reads that book once it is on show, as in Beta 3, which opened the book before any input. Another book, another screen or action first, a failed open or more than 10 seconds drop it.
