### Added

- **Interface 7 grew for Beta 4, draft 37** (#242). For plugin authors; plugins built for
  draft 36 run unchanged (no op id, record layout or snapshot byte of theirs moved), and are
  rebuilt only to use what is new:
  - a frozen service schema can gain ops: a revision only appends, a plugin built against an
    older or newer revision of a service still loads, and an op the opened revision lacks is
    refused as unsupported (`Service::open_newest`, `Service::revision`, `Service::has_op`;
    `docs/abi/stages-services.md` 2.3);
  - a format can read only some books of its extensions and hand the others on to the next
    format (`FormatDecl::asks`, `FormatCall::claimed`; `docs/abi/document.md` 1), for example
    a format of picture books for EPUB beside the built-in one;
  - a book can mix whole-page picture sections with reflowed ones
    (`FormatFeatures::some_whole_pages`, `DocState::some_whole_pages`, `BlockReply::whole_page`;
    each `SectionEntry::whole_page_picture` says which);
  - a download can ask never to take the larger buffers (`Request::priority` with
    `Priority::Lean`; `Priority::Boost` is `Request::foreground`, `Priority::Normal` the device's
    own choice as before).
