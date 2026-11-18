### Fixed

- **A large picture's page peaks 3 KiB lower** (#316). Shrinking a picture no longer keeps a table with an entry for each pixel of a row (3.2 KiB for a 1600 pixel wide picture): the pixel each one goes to is stepped along the row.
