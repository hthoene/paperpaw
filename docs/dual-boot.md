# PaperPaw beside another firmware (dual boot)

Some tools put two firmwares on one X4, each in one of the two app partitions
(`ota_0` and `ota_1`), and start the one the reader picks. PaperPaw updates
itself by writing the *other* app partition and switching the OTA data (the
table that says which partition starts). Beside another firmware that would
destroy it. So PaperPaw must be told that the other partition is not its own.
This page says how, for any tool.

## What the reader can do

Write this in `paperpaw.ini` on the card:

```ini
[firmware]
dual-boot = on
```

- `on`: this is a dual-boot setup.
- `off`: it is not, whatever else is found (a bootloader that carries the
  marker below is then ignored).
- `auto` (the default, and anything not set): the marker decides.

A value that is none of these (`dual-boot = onn`) counts as `on`: a typo must
not leave the other firmware open to an update. `dual-boot = off` undoes it.

Where a device is taken for a dual-boot one that is none, the *Updates* page
has a button, *This is not a dual-boot setup*: after a question that warns that
an update can overwrite a second firmware, it writes `dual-boot = off` here,
takes the note away and restarts. Where the file says `off`, the page offers
*Detect dual boot again*, which writes `dual-boot = auto` and restarts (or
delete the line by hand). The dualbooter's own bootloader is recognised by its
tag `x4boot` as a word of its own; the bootloaders PaperPaw ships and a plain
X4 runs are scanned in a test (`crates/paperpaw-x4/tests/bootloaders/`,
`PAPERPAW_BOOTLOADER_DIR` runs it on more) and none is taken for one.

## What a tool can do

Carry the string `paperpaw-dualboot:1` anywhere in the bootloader's window of
the flash, from 0x0 up to the partition table at 0x8000 (a constant in the
bootloader's image is enough). It says: *I manage the two slots; PaperPaw,
never write the other one, nor the OTA data.* PaperPaw reads the 32 KB of the
window (and `paperpaw.ini`) only where it matters: at a start that would write
the OTA data, at a start after one that found the setup declared (the note below
is on the card), and when an install is asked for. A normal start of a setup
that is not declared reads neither. It never writes before it has looked: where
the window cannot be read, it counts as marked.

PaperPaw cannot sit well beside a firmware whose tool does not tell it, and
phase 2 of this work will look for another firmware's image in the other
partition by itself and ask before replacing it. The marker stays the
surest way.

For the tool's own side we recommend: put PaperPaw in the slot that the plain
power button starts (waking from sleep is a reset, and the plain button
brings PaperPaw back); write PaperPaw's app image (`paperpaw-x4-<version>-install.bin`
of a release) raw, nothing else; keep both slots at least as large as the image
(the native images are over CrossPoint's 6,553,600-byte slots: write them to slots of PaperPaw's own
size, 8,323,072 bytes); keep the OTA data valid.

## What PaperPaw does when it is declared

- Never writes the other app partition: no update from the card, from the
  update server, nor the way back to another firmware.
- Never writes the OTA data at a start: no trial of a new firmware, no
  "undo" of an update that did not start, no marking of a failed one and no
  going back. A start looks at the OTA data first and writes nothing; the
  card, `paperpaw.ini` and the bootloader are read next (before the screen,
  the kernel and the plugins), and only then, if the setup is not declared,
  does the start do what it needs. Two things are marked at once, before the
  card: an update PaperPaw installed itself (its OTA entry carries PaperPaw's
  own word, which a tool's does not), and an update it had put on trial that
  failed. So a hang in the board's start still ends in the way back for a
  PaperPaw that is not told it shares the device. What a tool chose (`New` or
  `Undefined`, no word of PaperPaw's) waits: a hang before the card is read
  leaves it as it was (the bootloader's own rollback, where it has one, still
  applies).
  Where the card cannot be read at all, `paperpaw.ini` cannot be, and the
  start goes on as for a setup not declared by the reader.
  The first update from 0.6.0-beta.1 (made by a PaperPaw that does not put
  its word in the entry) carries no word of PaperPaw's either: that one start
  waits for the card and has no early rollback; the updates after it are
  marked at once.
- Once an install is asked for and the setup is found declared, it stays
  declared until the next start, even if `dual-boot` is switched off meanwhile:
  the safe direction.
- Never reads the other partition for the web flasher's bundle of plugins.
- Says so where an update would have been offered: the *Updates* page tells
  in two plain sentences that PaperPaw shares the device and installs no
  updates here (the details are in this file, not on the screen). It
  leaves the note `.paperpaw/boot.txt` on the card (`dual-boot source=ini
  slot=1`) for plugins; the note is removed when PaperPaw is not declared
  any more.

What stays: reading, plugins from the store (code plugins live behind the
image in PaperPaw's own partition) and everything on the card.

To update PaperPaw in such a setup, use the tool that set the two up.
