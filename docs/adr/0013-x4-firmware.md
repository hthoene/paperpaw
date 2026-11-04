# 0013. The Xteink X4's firmware

- Status: Accepted; the X4 Pro is built from the same firmware crate
  (feature `x4-pro`), and the UC8179 driver is in `paperpaw-x4`. A
  build for the X4 Classic (feature `x4-classic`) is there too, not part
  of the beta. What the Consequences list as still to come is done:
  Wi-Fi, lighter sleep, four grays, the UC8179 and the whole flash
  mapped.
- Date: 2026-10-15

## Context

The Xteink X4 is PaperPaw's smallest target: an ESP32-C3 (RISC-V, about
400 KB of RAM, 16 MB of flash) with a 4.26" 800 × 480 panel driven by an
SSD1677 controller, seven buttons, a microSD card and a battery. Its
hardware is documented by the community that writes open firmware for it
(the MIT-licensed [community SDK](https://github.com/open-x4-epaper/community-sdk),
[CrossPoint](https://github.com/crosspoint-reader/crosspoint-reader) and
the [FreeInk SDK](https://github.com/Free-Ink/freeink-sdk)): pins,
controller commands, button levels, the battery divider and the power
latch.

The hardware is not at hand while the firmware is written. Its parts must
therefore be testable without it, and the firmware must be thin enough
that what remains untested is little.

Related devices differ: the **X4 Pro** is an ESP32-S3 (Xtensa) with touch
and a front light, and newer batches of the X4 may carry an UltraChip
UC8179 controller instead of the SSD1677.

## Decision

- **Three crates.**
  - `paperpaw-x4` (`no_std`, published): the display, the buttons and the
    battery against `embedded-hal`, tested on a computer against a
    simulated controller.
  - `paperpaw-fat` (`no_std`): PaperPaw's `Storage` on a FAT file system
    with long file names, and a disk of 512-byte blocks as bytes; tested
    on disks in memory. Its FAT library is not released with `no_std`
    support yet, so it is pinned to a commit and not published.
  - `shells/xteink`: the firmware, built for the chip only (outside the
    workspace). It connects the chip's pins to the two crates and runs the
    kernel with the standard app.
- **No frame in RAM.** The kernel draws in bands of 96 rows, aligned to 8
  (`KernelBuilder::row_align`), and the display writes each band turned
  into the controller's memory. For a fast refresh, which compares with
  the frame shown, the bands go into the memory of the new frame, then
  after the refresh again into that of the frame shown
  (`Kernel::rerender`). Drawing twice costs time after the page is shown,
  not memory; a full frame would take a quarter of the heap.
- **Buttons are polled** every 10 ms, read from two resistor ladders by the
  ADC, and debounced; the kernel recognises long presses as on every
  device.
- **Sleep is power off.** After the sleep screen the firmware releases the
  battery latch (GPIO 13), which cuts the power on battery; the power
  button starts the X4 again, and PaperPaw reopens the book. On USB power
  it sleeps deeply until the button. A power button still down at start
  counts only once released.
- **Fonts as packs in flash**, up to 36 px: code and data share a 4 MB
  window of the flash.
- **The SSD1677 first.** A driver for the UC8179 follows once a device
  shows which controller it has; the display is the only part that
  differs. The X4 Pro gets its own firmware (another chip and toolchain),
  sharing the crates where its hardware is the same.

## Consequences

- Almost all of the firmware's logic runs in CI and on every computer;
  what needs the device is the wiring in `shells/xteink` and the panel's
  behaviour (waveforms, timing).
- Before the first flash, the stock firmware is saved (see
  `shells/xteink/README.md`), so the X4 can always go back.
- Still to come on the device: Wi-Fi (time, sync, transfers), lighter
  sleep between button reads (power), 4 gray levels, the UC8179 and a
  larger flash window.

## Later: the X4 Pro (2026-10-19)

The X4 Pro did not get a firmware of its own after all: `shells/xteink`
builds for either device, chosen by the feature `x4` (the default) or
`x4-pro`, so the main loop, the network, updates and remote control stay
one. What differs is the board (`board_pro.rs`), the card on the
ESP32-S3's SD host (`card_pro.rs`), the front light (`light.rs`) and the
drivers in `paperpaw-x4` (`uc8179`, `gt911`, `cw2017`, `bm8563`). Its
8 MB of PSRAM is the heap, so its UC8179 keeps a whole frame; its touch
controller also reports the home key below the screen. Its power latch
is held through resets (a pad hold), so it stays on while flashed.
Measured on the device: a UC8179 (its VER register's third byte 0x01;
Papyrix also knows X4 Pros with a UC8279, from 0x02, 0x68 or 0x69, which
PaperPaw does not drive yet).

## Later: the controller found at start (2026-10-20)

Xteink ships its devices with different display controllers, batch by
batch, and a driver for one shows nothing on another (an SSD1677's waits
for its busy line to go low, which an UltraChip's holds high at rest).
What the community documents (the FreeInk SDK, which reads the stock
firmware, and Papyrix and Launcher after it, all MIT): the first X4 Pros
have an SSD1677, later ones a UC8179 or a UC8279; the X4 Classic (an
ESP32-S3 board with other pins) has any of the three. No ESP32-C3 X4 with
an UltraChip has been reported, though these projects build their
UltraChip drivers into their X4 firmware too.

- **Asked at start, before the SPI bus takes the panel's lines**
  (`paperpaw_x4::probe`, the boards' `probe` module): the panel answers on
  the line it is sent on, so the firmware drives the lines itself and
  reads the UltraChips' status (`FLG`, 0x71) and revision (`REV`, 0x70),
  and looks at the busy line at rest. An SSD1677 holds it low and
  answers nothing; an UltraChip holds it high and answers, the revision's
  third byte naming the panel (the stock firmware's panel table: 0x01 and
  0x40 a UC8179, 0x02, 0x03, 0x41, 0x42, 0x67 to 0x69 a UC8279). Two of
  the three signs name an UltraChip; an SSD1677 needs all of its own.
  **Anything else keeps the driver the device always had**: the X4 its
  SSD1677, the X4 Pro its UC8179. The answers are logged at start
  (`display: UC8179 (FLG 13, REV 00 00 01 FF FF, BUSY high at rest),
  driven as UC8179`), all a report from a device with another
  controller needs.
- **One driver per controller family**, chosen by `paperpaw_x4::panel`:
  `display` for the SSD1677 (unchanged; on the X4 Pro with its sides
  swapped, as the X4 Pro turns), `uc8179` for both UltraChips. The
  UC8279 differs in tables, as the FreeInk SDK records them from the X4
  Pro: its panel setting (set again after the power comes on, which
  resets it), a one-byte border setting, its frame rate, the panel on the
  last 480 of its 600 lines in order, a fast refresh only within a
  window. Its gray waveforms are not public: on a UC8279, grays are shown
  in black and white for now.
- **Without the memory for a frame** (an X4 with an UltraChip), the
  UltraChip driver writes the bands into the controller's two frames as
  the SSD1677's does, each through a window of its memory (`PTL`, as
  GxEPD2 and the FreeInk SDK write parts of a frame), the layout of a
  band shared with the SSD1677 (`bands`). Its four grays need the frame
  shown in black and white first, then the grays' codes, two passes the
  bands cannot give: grays are shown in black and white there.

Not tried on a device yet: no device with these controllers is at hand.
What is checked: the probe against simulated panels, every sequence and
where each band lands against simulated controllers, the SSD1677's and
the X4 Pro's UC8179's bytes unchanged. To check on one: that the log line
names the controller, then a page turned fast and fully, the screen
turned, sleep and waking, and (on a UC8179 with the frame kept) grays.

## Later: the X4 Classic (2026-10-20)

The X4 Classic ("X4 v2", X4C) is an ESP32-S3 board, not a revision of
the X4: an ESP32-C3 image cannot run on it. The same firmware builds for
it with the feature `x4-classic` (`cargo +esp x4classic`); `x4-pro` and
`x4-classic` share what both ESP32-S3 devices build with (the feature
`esp32s3`: the chip, its PSRAM as the heap, the card on its SD host,
`card_pro.rs`, outer power seen by the USB frames, `usb_power.rs`).
Its board (`board_classic.rs`) has what differs, from the FreeInk SDK's
board table and Papyrix's notes (both from the stock firmware, MIT):

- **The panel on other pins** (data/command GPIO 14, reset GPIO 10, busy
  GPIO 18), the glass and memory of the X4 Pro (`Device::X4Pro`: a frame
  kept, turned as the X4 Pro's), its controller asked at start like the
  others' and driven as such; without a sure answer, a UC8179.
- **The X4's seven buttons, each on a pin of its own**, low while
  pressed (`State::keys`): back, confirm, left and right below the
  screen (with the X4's row of hints), the page buttons on the side,
  power. No touch, no front light.
- **The card's power** on GPIO 6 (on while low), off for 80 ms first as
  the stock firmware does; the gauge and the clock chip as on the X4 Pro.
- **Its own id** (`xteink-x4-classic`, plugin libraries' `x4-classic`):
  plugins made for the X4 or the X4 Pro are not offered to it until the
  library lists it.

Not tried on one: no X4 Classic is at hand. To check: the log line
naming the controller, every button (the side buttons' direction above
all, which the sources name differently), the screen's turning, the card,
the battery's charge (the gauge may need its battery profile, which the
stock firmware writes), whether the charging pin reads as on the X4 Pro,
sleep and waking by the power button.

Update (2026-10-25), after a cross-check of the board with the FreeInk
SDK (44427f8d) and Papyrix (fb09e50f): where both agree, the board now
does as they say, since it cannot be tried. The panel is driven at
10 MHz as `Device::X4Classic` (the X4 Pro's, but a UC8279 keeps its
factory frame rate: no PLL command); its controller is the one the
factory's NVS names (`hw_calib`, `screenType`), else the one that
answers (its reset held 50 ms, as the stock firmware does), else a
UC8279, never a blank screen. The charging pin (GPIO 21, no pull) is
high while charging. The card's power goes off 80 ms and on 120 ms
before each of up to four starts. In the deep sleep GPIO 1 stays high,
the panel's reset high and the card's power off, each held; the X4
Classic has no other off. Its partition table (`partitions-classic.csv`)
keeps the factory's NVS (0x9000 to 0xE000) whole, the OTA data after it
at 0xE000. The side buttons stay the FreeInk SDK's (GPIO 0 up); key
settings swap them if they turn the wrong way.

Update (2026-11-04, #209): on the owner's X4 Pro the charging pin (GPIO 21, the board's pull-up kept) reads **low on battery and high on a computer's cable** with the cell charging (gauge 69 %, 4063 mV), so it is high while on power, like the Classic's; the first port had assumed low. Whether it says "charging" or only "power present" is not told apart by these readings (a full cell on a cable would). The X4's pin (GPIO 20) and the X3 (fuel gauge) are unchanged.
