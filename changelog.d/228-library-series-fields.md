### Added

- **`core.library` ops 6 `next_in_series` and 22 `fields` on the production host** (#228). `next_in_series` answers the lowest unread book of the book's series (the given book when it is that one), one catalog scan that holds a number and an offset. `fields` lists a book's fields of every plugin by plugin id then name, paged under a field generation that changes with the fields; the title of a row is the field's name until the shell supplies the translated one (`CatalogEnv::field_title`).
