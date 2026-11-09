### Changed

- **LIB op 24 `provide` is checked as the contract says** (#228). `flags` follow the record's field order (bit 0 title, 1 author, 2 series, 3 number, 4 tags) and must name at least one field, none above bit 4. A provided text that is blank (Unicode White_Space, as `str::trim`), a set bit with no value, a tags bit with no tag, a blank or empty tag line, and any control character (`char::is_control`: U+0000-U+001F, U+007F, U+0080-U+009F) or U+2028 or U+2029 in a title, author, series, number or tag are `ERR_INVALID`; more than 32 tags stay `ERR_NO_SPACE`. Nothing is escaped.
