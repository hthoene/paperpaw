### Fixed

- **The EPUB sleep-parity case runs again** (#243, todo 617). A parked plugin made by an input asks for one more pass at once and the pass after it sets the next wake-up; the test now folds that tail pass into the one before it and counts it (awake and asleep may differ by at most 2 per step), where it compared the first effect only. No change to the firmware.
