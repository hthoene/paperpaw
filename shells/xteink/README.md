# PaperPaw on the Xteink X4

The firmware for the Xteink X4 and the X4 Pro, in beta with
0.6.0-beta.4. See
[ADR 0013](../../docs/adr/0013-x4-firmware.md) for how it is built; its
hardware parts are in [`paperpaw-x4`](../../crates/paperpaw-x4) and
[`paperpaw-fat`](../../crates/paperpaw-fat), tested on a computer.

> Save the stock firmware first (below), so the X4 can always go back.

It is for the X4 and, built with the feature `x4-pro`, for the X4 Pro
(ESP32-S3); the display controller (SSD1677, UC8179 or UC8279) is
found at start. A build for the X4 Classic comes with each release too,
experimental ([below](#the-x4-classic)), and one for the Boox Picco, not
yet tried on a device ([below](#the-boox-picco)).

## Build

The Rust release pinned in this folder's `rust-toolchain.toml` builds
it (rustup fetches it, with the target `riscv32imc-unknown-none-elf`):
native code plugins run only on firmware built by the same compiler
(ADR 0026), so each release names its toolchains; `versions.env` names
the X4 Pro's. mbedTLS is built with the firmware (only what its HTTPS
client uses, see `Cargo.toml`): that takes CMake, Ninja and, for the X4,
clang (`versions.env` names the version CI uses), found on the `PATH`
or by `CLANG_PATH`, with `CMAKE_GENERATOR=Ninja` set.

```sh
cd shells/xteink
cargo build --release
```

## Save the stock firmware

Install [espflash](https://github.com/esp-rs/espflash)
(`cargo install espflash`), connect the X4 by USB and read its whole
flash (16 MB) into a file. Keep that file.

```sh
espflash read-flash 0x0 0x1000000 x4-stock-firmware.bin
```

To go back to it later:

```sh
espflash write-bin 0x0 x4-stock-firmware.bin
```

## Flash and run

```sh
cargo run --release
```

flashes the X4 and shows its log. Books go on a microSD card formatted
with FAT32, e.g. in `Books`. PaperPaw does not read exFAT, which large
cards often come with.

[`tools/flash-x4.py`](../../tools/flash-x4.py) does the same for each reader
(`python tools/flash-x4.py`, or `x4-pro`, `x4-classic`, `x3`): it says
which branch and commit it builds, checks the tools, can save the reader's
flash first (`--backup`), asks before flashing, and takes `--port`,
`--no-monitor` and `--dry-run`.

## From CrossPoint's menu

Each release has an install file for every device,
`paperpaw-<device>-<version>-install.bin`: the app image alone,
unsigned, as CrossPoint's installer takes it.

- **From CrossPoint to PaperPaw**: from Beta 4 PaperPaw installs by flashing
  (paperpaw.org/flash, or `tools/flash-x4.py`), which also writes PaperPaw's
  own partition table (app slots of 7.9 MB). The install file still goes
  through CrossPoint's *Settings*, *General*, *SD firmware update* when it
  fits CrossPoint's app slot (6.25 MB): CrossPoint writes it into its other
  slot and starts it; PaperPaw starts on trial and, if it fails, CrossPoint
  comes back. The native images are larger than that slot: the build tells
  how much over it they are, and the menu cannot install them. A device that
  keeps CrossPoint's layout runs PaperPaw as it finds it (it never writes the
  partition table), cannot take an update larger than its slot (the update
  page says so before downloading, Decision 179), and has little or no room
  for code plugins from the store (the store says so).
- **Back to the stock firmware by card** (the X4's *Up* and *Power*
  with `update.bin`) works only while the stock bootloader is still in
  the flash: a device that got PaperPaw by the menu keeps it, one
  flashed whole from a computer or the web flasher does not. Without it,
  the way back is the saved flash (above).

## Remote control

Over USB, [`tools/x4-remote.py`](../../tools/x4-remote.py) presses the
X4's buttons, takes screenshots and prints what the screen shows, with
the steps of the emulator's scripts. Firmware built here has it (the
`remote` feature, on by default); released firmware leaves it out. The
transfer over the cable, which installs plugins and adds books once the
reader allows it, is in every firmware
([docs/usb-transfer.md](../../docs/usb-transfer.md)).

It also waits for and checks what the screen shows (`until:`, `expect:`,
with an exit code for scripts), taps and swipes (the X4 Pro), runs
actions, sends events, reads and sets settings and the clock, lists the
plugins, actions, settings and screens open, says what each plugin took
(`usage`), works with the card (folders pushed and pulled, files checked
by their MD5, `ls`, `rm`, `mv`), changes the log's level, restarts the X4
or makes it panic and waits until it is back. `--help` lists it all; the
protocol is in `paperpaw_x4::remote`.

Sleep enters hardware sleep even with USB connected, in release and
test builds. The USB link stops until the physical power button wakes
the device; remote commands cannot wake a sleeping processor. Finish
captures before requesting sleep, then reconnect after the button press.
Idle light-sleep tests (`idle sleep`) remain time-bounded and restart
the device after the requested interval.

```sh
python tools/x4-remote.py --script "page-forward,right,describe" --screenshot out/x4.png
python tools/x4-remote.py --script "run:update.open,until:Search online,expect:Wi-Fi transfer"
python tools/x4-remote.py --push my-fonts .paperpaw/fonts --info
python tools/x4-remote.py --script-file tools/pps/smoke.pps --out out/smoke
```

Script files (`.pps`) add variables, loops, measurements, checks and a report, and run
unchanged on the emulator (`--emulator`): [`docs/device-test-scripts.md`](../../docs/device-test-scripts.md).

## Back to another firmware

Put the other firmware's file (`.bin`, CrossPoint's for example) at the
top of the card or in `updates` and open *Update*. PaperPaw shows it as
another firmware, not signed, and installs it only when chosen
(*Install anyway*). It checks that the file is a whole app image for
this device (its chip, segments, checksum and hash, and CrossPoint's
board tag where it has one), writes it into the other app slot and
starts it; the running firmware is not touched until then.

## What works, what is to come

- The screen (black and white, fast and full refreshes), the seven
  buttons, the battery, the SD card with long file names, sleep (the
  power button puts it to a light sleep where a press wakes it at once; it
  switches off after a while asleep, 30 minutes by default, and the next
  press starts it again; the X4 Pro does the same, with its front light
  off meanwhile; the X4 Classic keeps its deep sleep), light sleep
  between button reads on battery.
- Wi-Fi (ADR 0016): on when something needs the network, chosen on the
  Wi-Fi page in the settings; HTTP and HTTPS (ADR 0017) for plugins,
  downloads straight to the card.
- Updates from the card (ADR 0019): two app partitions
  (`partitions.csv`, which `espflash.toml` names), signed update files
  made with `paperpaw-sign`, a failed update undone at the next start;
  or fetched over the network by the `update` plugin.
- The UltraChip UC8179 of newer X4s as well as the SSD1677, whichever
  answers at the start.
- Code plugins as files on the card (ADR 0024). After failed starts, or
  with *Back* held at power-on, PaperPaw starts in safe mode without
  them. On the X4 Pro, hold *Home*.
- A crash is written to `.paperpaw/crashes.txt` on the card, for bug
  reports.
- The time of day from the network. The X4 forgets it while it is off.
- Wi-Fi set up with a phone, or the network the web flasher was given.

## The X4 Pro

Its chip is an ESP32-S3 (Xtensa): install the `esp` toolchain with
[espup](https://github.com/esp-rs/espup), in the versions of
`versions.env` (`espup install --targets esp32s3 --toolchain-version
1.97.0.0 --crosstool-toolchain-version 14.2.0_20241119`; newer GCCs
reject esp-hal's linker scripts), load its environment (`export-esp.ps1`
or `export-esp.sh`), then

```sh
cargo +esp x4pro          # build
cargo +esp x4pro-run      # flash and show the log
cargo +esp x4pro-clippy   # lints
```

The first flash over the stock firmware needs its power button held
until the flashing ends (its boot loader does not keep it on); PaperPaw
holds its power through resets, so later flashes need nothing held. Its
USB comes through the pogo pins (Espressif's `303A:1001`).

What works: the screen (black and white, and four grays for pictures),
touch and the home key, the page and power buttons,
the SD card (SDMMC), the battery from its fuel gauge, the clock chip,
the front light (brightness and warmth, in the quick panel), sleep, the
network as on the X4, the UC8279 of other X4 Pros. To come: battery life
measured.

Bluetooth (ADR 0027) is included in X4 Pro builds: scanning, connecting,
GATT client reads, writes and notifications, pairing confirmed on the
reader, and HID page-turner or keyboard keys. The X4, X4 Classic and X3
do not enable it. Advertising and plugin GATT servers are not yet
implemented on the reader. The plugin interface remains i4.

Remotes default to **By hand**. The radio starts only when requested; a
remote enabled by hand reconnects after waking until it is switched off.
While Bluetooth is active, the processor cannot light-sleep, even while
a bonded remote is absent and being watched for. Switch it off when not
needed. Device sleep shuts Bluetooth down before hardware sleep; wake
with the physical power button. Automatic remote policies remain opt-in
until connected idle power and radio-compatible light sleep are measured.

The usual `cargo +esp x4pro` and `x4pro-run` aliases include Bluetooth.
The earlier `x4pro-bt` and `x4pro-bt-run` names remain compatibility aliases.

## The X4 Classic

Experimental: released with the others (`paperpaw-x4-classic-<version>.bin`),
but not yet tried on a real device.

The X4 Classic ("X4 v2") is an ESP32-S3 board like the X4 Pro, with the
X4's buttons and no touch or front light; an X4 image cannot run on it.
With the `esp` toolchain as for the X4 Pro:

```sh
cargo +esp x4classic          # build
cargo +esp x4classic-run      # flash and show the log
cargo +esp x4classic-clippy   # lints
```

It answers on USB (through its pogo adapter) as the X4 Pro does, so
check which one is connected before flashing. Its pins come from the
FreeInk SDK's and Papyrix's notes; not yet tried on one.

Its partitions are its own (`partitions-classic.csv`, which
`x4classic-run` and the release use): the factory's NVS, which names the
panel's controller, stays whole from 0x9000 to 0xE000, and the OTA data
follows it where the stock firmware has it. An image merged for the
whole flash covers that NVS with erased bytes too, so a flasher writes
its parts apart (or backs the flash up first).

## The Boox Picco

Experimental, and **not yet tried on a device**: PaperPaw's team has no
Picco. It is built (`cargo +esp picco`) and its image is part of a
release, `paperpaw-picco-<version>-install.bin`. It may show nothing,
and it may not start at all.

> **Save the stock firmware first.** Onyx publishes no image of it; the
> only way back is a copy of the Picco's own flash, read before the first
> install (below). Onyx has reportedly refused warranty for Piccos with
> another firmware.

The Picco is an ESP32-S3 with 16 MB of flash and 8 MB of PSRAM, as the X4
Pro, and is built as one (the feature `picco` is `x4-pro` with the board
`board_picco.rs`): a 3.97" panel of 800 × 480, touch, a warm and cool
front light, two side buttons and power, no home key. Its pins and parts
come from the FreeInk SDK and CrossPoint Reader (both MIT), who read them
from the stock firmware.

```sh
cargo +esp picco          # build
cargo +esp picco-clippy   # lints
```

PaperPaw goes into the stock firmware's app partition alone, at
`0xE0000`. Onyx's boot loader, partition table, settings and recovery
stay as they are; **never write a whole-flash image of another device
to a Picco, and never flash it with `cargo run`**. With a cable (the
Picco enters its download mode by itself; if it does not connect, try
again with it awake):

```sh
# Once, before anything else: the whole flash into a file. Keep it.
espflash read-flash --chip esp32s3 0x0 0x1000000 picco-stock-firmware.bin
# Secure boot and flash encryption must be off (they are on retail units):
espefuse.py --chip esp32s3 summary | grep -E "SECURE_BOOT_EN|SPI_BOOT_CRYPT_CNT"
# PaperPaw:
espflash write-bin --chip esp32s3 0xE0000 paperpaw-picco-<version>-install.bin
# Back to the stock firmware:
espflash write-bin --chip esp32s3 0x0 picco-stock-firmware.bin
```

A build of your own is made into that file with `espflash save-image
--chip esp32s3 --flash-size 16mb --partition-table partitions-picco.csv
--target-app-partition app <the ELF file> paperpaw-picco-install.bin`.

What differs from the X4 Pro, and what is not known without a device:

- its panel is one of four by unit. Three have an SSD1677, driven as
  the X4's with the panel's own waveforms (the stock firmware loads
  waveforms of its own for each; whether the panels' own look right is
  not known); the fourth (SE0400NQW47, "SSD2677") has a driver of its
  own with the stock firmware's black-and-white waveforms and shows no
  grays. Which way the screen lies turned on its side is taken as the
  X4 Pro's;
- one app partition: PaperPaw cannot update itself on a Picco (*Update*
  says there is no room); a newer version is flashed as above. The
  store's plugins are the X4 Pro's;
- the charger is only read: PaperPaw does not raise its charge voltage
  as the stock firmware does (4.35 V), so the cell charges to the
  charger's own setting and may show less than 100 % when full;
- it sleeps as the stock firmware does (its power held, its parts' power
  off) and wakes by the power button. Safe mode is the upper side button
  held at power-on;
- the slide switch and the motion sensor are not used. Wi-Fi and
  Bluetooth are the X4 Pro's code; CrossPoint has open reports of Wi-Fi
  not joining on the Picco.

## The X3

Experimental: built (`cargo x3`), but not yet tried on one, and not
part of a release.

> **Only for unlocked X3s.** Units from xteink.com are not locked; some
> sold elsewhere are, and need the Xteink Unlocker, which officially
> supports only CrossPoint and CrossInk. Another firmware on a locked
> unit may brick it or leave it with no way back, and the maker's
> warranty excludes damage from unofficial firmware.

The X3 has the X4's chip (ESP32-C3), bus, card and buttons, a 3.68"
panel of 792 × 528 with a UC8253 controller, and on I²C (data GPIO 20,
clock GPIO 0) a BQ27220 fuel gauge and a DS3231 clock. It is a build of
its own (the feature `x3`, `board_x3.rs`), not found at start:

```sh
cargo x3          # build
cargo x3-run      # flash and show the log
cargo x3-clippy   # lints
```

Its facts come from the FreeInk SDK (MIT); the UC8253's start and its
waveform tables derive from the OpenX4 E-Paper Community SDK (MIT), with
their notice in `crates/paperpaw-x4/src/uc8253.rs`. It takes the plugin
library's X4 builds (the same chip and fingerprint). Not yet known
without a device, and marked so in the code:

- GPIO 13 is the card's power rail (as Papyrix has it), on from the
  start and off only before deep sleep, the X3's off; not proven on a
  device;
- which way the panel lies and whether it is mirrored (taken as the
  X4's), and whether partial windows and its 10 MHz bus behave as the
  SDK's;
- whether its buttons' levels and order are the X4's;
- it counts as on USB power while the fuel gauge says the battery
  charges or the chip's own USB is connected (a full battery on the
  cable has no current); how that behaves on a charger alone is not
  tried;
- the fuel gauge forgets the cell's capacity when it loses power and
  says TI's 3000 mAh: the firmware writes 650 mAh back at the next start
  (some seconds, then sealed again), as the FreeInk SDK and Papyrix do;
- units with the newer UC8279d controller (from about July 2026) are
  driven with that controller's own set-up and waveforms (the FreeInk
  SDK's); tried on one real unit only after the first beta 4 test.
  Grays fall back to black and white, as on the UC8253;
- the tilt sensor (QMI8658) is left out, and NFC, which the maker
  mentions, is not used.
