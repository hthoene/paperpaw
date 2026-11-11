# X4 test image: steps for the owner

This is the **test image** (decision 156, #243): the native plugins are on, and
passwords (Wi-Fi, lock, settings) are only **veiled**, not encrypted. It is not
a release and must not be given to anyone else. **You flash it; no agent does.**
Nothing here has been run on a device. Steps marked *(pending #2462)* are
filled in when Senior's conditions have landed.

> **Note your Wi-Fi/account passwords before flashing; you'll need to re-enter
> them later.** The first time the test image writes a secret it re-veils *all*
> the password lines Beta 3 saved. Under the real encryption that comes later, or
> after going back to Beta 3, every password on this X4 (Wi-Fi, accounts) is gone
> and has to be typed in again. Your books and the rest of the settings stay.

What you need: the X4, a USB cable, a charged battery (above 50 %),
[espflash](https://github.com/esp-rs/espflash) (`cargo install espflash
--version 4.6.0 --locked`; the bundle was made with the version in
`BUILD-INFO.txt`), and this folder.

## 1. Check the bundle

```sh
sha256sum -c SHA256SUMS            # every line must say OK
# Windows PowerShell:  Get-FileHash <IMAGE> -Algorithm SHA256   (compare with SHA256SUMS)
```

Stop if a line says FAILED. The commit and the features are in `BUILD-INFO.txt`
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

- PaperPaw starts and shows the home screen. *(pending #2462: first-boot notes)*
- Settings › About (or Diagnostics) shows the line **"Secrets: veiled (test build)"**.
- The boot log (`espflash monitor`, then reset the X4; the test image has no USB remote control)
  prints `secrets: veiled (test build)`.
- Open a book, turn some pages, put the X4 to sleep and wake it again.
- Wi-Fi: connect once, restart the X4, check it still has the password.

Please note anything that looks wrong, with the log lines if you can.

## 5. Go back

```sh
espflash write-bin 0x0 x4-before-test-image.bin
```

This writes back the whole flash you saved in step 2 (including which app slot
starts). If the X4 does not react to the cable at all, hold the power button
while it connects, as for the first flash of an X4 Pro.

## If it does not start

- A screen that never changes, or restarts in a loop: unplug USB, hold power for
  10 s, try again; if it loops again, do step 5 and send the log
  (`espflash monitor` right after a restart). *(pending #2462:
  boot-loop guard / safe mode description)*
