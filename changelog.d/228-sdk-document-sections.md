### Added

- **Section lengths of an open document** (#228). `Documents::sections` reads `core.document` op 5 in bounded pages (`DocPage<SectionEntry>`): each `SectionEntry` has the section's `index`, its `length` in source units and `whole_page_picture`, so a reader can compute progress where the format has no progress of its own. The whole page is validated before any row is read, like the table of contents.
