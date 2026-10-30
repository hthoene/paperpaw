# 0019. Firmware updates

- Status: Accepted
- Date: 2026-10-18

## Context

The X4's firmware is PaperPaw with all its code plugins built in. Until
now a new version needed a computer: the web flasher or espflash over
USB. Readers should update on the device, and nothing about an update
may leave a device that does not start.

- The X4 has 16 MB of flash; the firmware takes 5.3 MB.
- The bootloader espflash brings (ESP-IDF's, `release/v6.1`) chooses
  between app partitions by the OTA data partition and skips images
  marked bad, but it is built without rollback: it does not mark a new
  image on trial, nor undo one that never confirmed itself.
- Anyone in the same network, or anything that changes a file on the
  card, could otherwise put firmware on a device.

## Decision

- **Two app partitions of 7.9 MB** (`shells/xteink/partitions.csv`),
  chosen by the OTA data partition. The web flasher writes them once;
  from then on updates come without a cable. `cargo run` erases the OTA
  data, so the firmware flashed is the one that starts.
- **The firmware undoes a failed update itself** (`crates/paperpaw-x4/src/updates.rs`,
  which `shells/xteink/src/ota.rs` runs on the chip's flash; a computer tests the
  whole path in `crates/paperpaw-x4/tests/update_path.rs`).
  A new image's first start marks it on trial, before the board is set
  up; running for half a minute in all marks it good. Going to sleep or
  restarting before marks it new again with the time it ran (in the OTA
  data entry's label, which the bootloader does not read), so the next
  start, on the X4 also a wake from sleep, goes on with the trial. An
  image found on trial at a start (its last run ended otherwise, e.g. it
  panicked or hung until the watchdog ended it) is marked bad and the
  device restarts; the bootloader skips it, and the firmware before it
  takes its place back and says so. These marks go to the running
  image's entry, also when an update was chosen meanwhile; choosing
  writes the other entry whole, so a power cut keeps the choice before.
- **Signed update files** (`paperpaw-update`): the app image followed by
  a trailer with its length, the signer's Ed25519 public key and the
  signature over both. The X4 checks the signature while it writes the
  image, a piece at a time, and chooses the new image only if it holds;
  the image's first word, without which the bootloader starts nothing
  there, is written only then. An update installed and not started yet
  is given up before another is written, so nothing is ever written into
  the partition chosen to start.
  It installs images signed with PaperPaw's key, built in, or with a key
  the reader accepts at that install, shown its fingerprint (their own
  builds); one not newer than the running firmware only when the reader
  chooses it, told it is older. Nothing on the card trusts a key.
  `paperpaw-sign` makes keys, signs images and shows what a file holds;
  releases are signed with a key kept as a secret of the repository.
  The whole flash image the web flasher writes is signed with the same
  key, in a signature file of its own (`.sig`, the trailer alone); the
  website checks it when it fetches a release, and the browser again
  before it writes.
- **The shell installs, a plugin asks** (`paperpaw_sdk::firmware`): the
  shell does what `Command::Firmware` says (install a file, restart) and
  reports its progress; the `update` plugin finds update files in
  `updates` on the card, says what they are and who signed them, offers
  a newer one at start, and says after the restart how it went.

## Consequences

- An update from the card takes about 45 s on the X4, its signature
  checked on the way; a damaged or changed file is refused before it is
  chosen.
- An image that does not start cleanly comes back by itself:
  tried on the X4 with an image that panics at start.
- Who can change the card cannot make a key trusted nor bring back an
  older firmware unasked; the reader is asked each time. Anyone with the
  device and a USB cable can flash it anyway.
- Devices flashed before the two partitions came need the web flasher
  once more; until then the update page is not offered.
- Updates over the network (from paperpaw.org) build on this: they only fetch the file.
