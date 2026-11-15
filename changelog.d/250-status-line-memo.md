### Changed

- **The reader's status lines ask the registry once per page** (#250). A status line drawn band by band asked the registry for its items at every draw. `paperpaw_ui_kit::status_line::draw_kept` keeps the answer in a `Memo` of the caller's between draws; the reader forgets it when its screen is shown again and at an input.
