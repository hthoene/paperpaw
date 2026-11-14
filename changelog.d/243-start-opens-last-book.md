### Fixed

- **The book read last opens in the first frame** (#243). The start drew the home screen first and the book one pass later, a flash and an extra full-screen refresh that Beta 3 did not have. The start now settles its first passes before it draws, so the book is the first frame (home only where there is no last book).
