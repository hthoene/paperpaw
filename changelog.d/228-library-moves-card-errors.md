### Fixed
- A library move no longer swallows a card write error while it carries a book's details, cover, recent and favourites place: the step fails with the card's status and is repeated (it went on, ended normally and left them under the old path). The replaced book's discard likewise (`library::try_moved` / `try_removed`).
