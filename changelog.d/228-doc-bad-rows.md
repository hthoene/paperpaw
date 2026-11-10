### Changed

- **A bad `doc_toc` or `doc_link` row is dropped, not the page** (#228). A format's table-of-contents row with a level over 15 or an unknown flag, or a link row with a kind over 2 or `end < start`, is left out of the page the opener gets, like a picture that fails its check; the other rows are delivered and the paging goes on. It no longer fails the page or counts against the document. A broken stream (rows out of order, a record that does not decode) still fails the page.
