### Changed

- **The X4 keeps the time through a restart** (#280). Until now the X4
  showed `--:--` and no date after every restart (an update, a crash, a
  wake from the deep sleep on USB) until Wi-Fi set the clock again, and
  reading statistics waited for it. The X4 now keeps the last time it was
  set in the chip's RTC and goes on from it: Home shows the time at once
  and the reading is dated. The kept time is approximate (the RTC's clock
  is a few per cent off) and Wi-Fi sets it exactly at its next chance.
  After a real power-off (battery out, or the switch-off without USB) the
  RTC is gone too: the time stays unknown (`--:--`) until it is set.
  Time read is counted by the monotonic timer, not by the clock.
