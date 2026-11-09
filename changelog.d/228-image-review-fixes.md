### Changed

- **Prepared pictures are bounded before they are decoded** (#228). A
  source over 8 Mpx is refused from its header, a prepared picture is at
  most 1,600 pixels on its longest side on every device, the image
  command paths count against the callback's 4,096-byte command budget,
  and the boot sweep of the picture cache reads its folder with the
  bounded listing. `ui::Picture` is `#[non_exhaustive]` and has
  `Picture::new`.
