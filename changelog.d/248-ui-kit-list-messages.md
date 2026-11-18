### Changed

- **Lists let their message buffer go before they paint their rows** (#248). `ItemList` kept the
  4 KiB catalog buffer of its header while drawing every row; it is now freed once the header is
  made, 4 KiB less at the peak of drawing a list (on the X4, the store's install).
