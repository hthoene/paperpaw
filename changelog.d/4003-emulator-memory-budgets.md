### Changed

- **Emulator memory tests count the reader's covered page** (#4003). Since the reader draws its page when a question or the store is over it (#359), the status-line frame test allows 13 KiB above live (12.7 measured) and the store tests 87 % of the heap (the store's emulator peak is 170 KiB, was 162.5).
