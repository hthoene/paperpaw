### Changed

- **The reading screen's settings moved to their own file** (#228). `native_view.rs` keeps under
  the length limit; the reading settings and layout key are in `native_view/setup.rs`. No change
  in behaviour.
