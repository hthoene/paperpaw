# X4 test image: steps for the owner

This is the **test image** (#243): the native plugins are on, and passwords (Wi-Fi, lock, accounts) are kept with the device's own key (the real sealer, #1224). It is not a release and must not be given to anyone else. **You flash it; no agent does.**
Nothing here has been run on a device.

> **Passwords stay; a backup of the flash is still advised** (step 2).

What you need: the X4, a USB cable, a charged battery (above 50 %),
[espflash](https://github.com/esp-rs/espflash) (`cargo install espflash
--version 4.6.0 --locked`; the bundle was made with the version in
`BUILD-INFO.txt`), and this folder.

## 1. Check the bundle

```sh
sha256sum -c SHA256SUMS            # every line must say OK
# Windows PowerShell:  Get-FileHash <IMAGE> -Algorithm SHA256   (compare with SHA256SUMS)
```

Stop if a line says FAILED. `BUILD-INFO.txt` also says whether the app fits CrossPoint's smaller
slot; if it does not, this image can only be flashed over USB, never from CrossPoint's menu. The commit and the features are in `BUILD-INFO.txt`
(features contain `test-image`, never `release`).

## 2. Save what the X4 has now (do not skip)

Connect the X4 by USB. Keep the file, 16 MB:

```sh
espflash read-flash 0x0 0x1000000 x4-before-test-image.bin
```

The microSD card is not touched by any step here: books and settings stay.

## 3. Flash

```sh
espflash erase-parts otadata --partition-table partitions.csv   # so the new image starts
espflash write-bin 0x0 <IMAGE>
```

Then press reset (or unplug and plug in). If `espflash` cannot reach the X4, see
"If it does not start" below.

## 4. What you should see

- PaperPaw starts and shows the home screen.
- The boot log (`espflash monitor`, then reset the X4; the test image has no USB remote control) shows the start; keep it if anything looks wrong.
- Every 30 s the log has a `high-water:` line (stack left, heap free, largest
  block); after a restart the first lines say `high-water: the run before …`.
  Copy those lines if anything looks wrong.
- Open a book, turn some pages, put the X4 to sleep and wake it again.
- Wi-Fi: connect once, restart the X4, check it still has the password.

Please note anything that looks wrong, with the log lines if you can.

## 5. Go back

```sh
espflash write-bin 0x0 x4-before-test-image.bin
```

This writes back the whole flash you saved in step 2. If it does not start
afterwards, run `espflash erase-parts otadata --partition-table partitions.csv`
once more. The card is untouched. If the X4 does not react to the cable at all, hold the power button
while it connects, as for the first flash of an X4 Pro.

## If it does not start

- A screen that never changes, or restarts in a loop: unplug USB, hold power for
  10 s, try again; if it loops again, do step 5 and send the log
  (`espflash monitor` right after a restart). The firmware has a guard: after two
  failed starts of the native plugins it starts **without** them (the log says
  so), so a loop that goes on is a different problem: send the log.
- A stack-overflow test is not part of this image; it needs your own approval.
