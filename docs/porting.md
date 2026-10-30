# Porting PaperPaw to another device

This guide is for adding a device to PaperPaw: another e-reader whose
firmware PaperPaw replaces, or a board of a device PaperPaw runs on
already. It follows how the Xteink X4 Pro and the X4 Classic were added
to the X4's firmware, step by step, and ends with what a pull request
for a new device brings.

A device with an operating system of its own (Android, Linux) gets a
shell of its own instead, as `shells/android` and `shells/desktop` are:
the same kernel and plugins, the system's files, input and screen. Most
of this guide is about firmware; its rules on what a port touches, its
on-device check and its checklist apply to shells too.

## What a port touches

PaperPaw keeps everything that differs between devices in the shell and
its driver crates. A port touches these:

| Part | Where | What it holds |
|---|---|---|
| The board | `shells/xteink/src/board_<name>.rs` (the X4's is `board.rs`) | Which pin and unit does what, in one place, with a table at the top |
| A cargo feature | `shells/xteink/Cargo.toml`, `shells/xteink/.cargo/config.toml` | The device's feature, its chip's crates, build aliases |
| The display controller | `crates/paperpaw-x4` (`display`, `uc8179`, `panel`, `probe`) | The controller's driver, tested against a simulated controller |
| Input | the board, `shells/xteink/src/input.rs`, `paperpaw_x4::buttons`, `gt911` | Buttons by pin or by ADC ladder, touch, their names |
| Power and battery | the board, `power.rs`, `usb_power.rs`, `paperpaw_x4::cw2017` | The power latch, the battery's charge, outer power, sleep |
| The card | `card.rs` or `card_pro.rs`, `volume.rs`, `paperpaw-fat` | The SD card's bus and its FAT or exFAT file system (`ExFatStorage`, ADR 0033) |
| The light | `light.rs`, `device.rs` | The front light's brightness and warmth |
| The clock | `clock.rs`, `paperpaw_x4::bm8563` | A clock chip that keeps the time while off |
| Wi-Fi | `wifi.rs`, `network.rs` | The radio, usually unchanged within a chip family |
| What the kernel is told | `main.rs` (`DeviceInfo`, `KernelBuilder`) | Id, name, screen, input, memory, capabilities, defaults |
| Updates | `ota.rs`, `partitions.csv` | The chip and the firmware's name each update is checked against |
| Code plugins | `shells/xteink/versions.env`, `rust-toolchain.toml` | The toolchain, and so the compiler and fingerprint plugin files must have |
| The emulator | `shells/desktop/src/profiles.rs` | A profile as the firmware builds the device |
| CI and releases | `.github/workflows/ci.yml`, `release.yml`, `.github/actions/xteink-tools`, and `xtensa-std` for an ESP32-S3 | The device in each matrix: lint, build, images, updates, plugins |
| Documents | ADR 0013, `shells/xteink/README.md`, `CHANGELOG.md`, `README.md` | What works, what was tried on a device, what is to come |

What a port **does not touch**: the kernel, the SDK and the plugins, in
`crates/paperpaw-core`, `crates/paperpaw-sdk` and `plugins/`. They know
no device by name and have no `cfg` for one. What they need to know
about a device comes from `DeviceInfo` and the kernel's capabilities,
such as `KernelBuilder::frontlight`, `wifi`, `sleep` and `button_row`.
The rest comes from probes a shell gives the kernel:
`KernelBuilder::clock`, `random`, `memory` and `started`. Where a device has something no capability or SDK
API says yet, the SDK gains one, so that every device and every plugin
can use it (see [CONTRIBUTING.md](../CONTRIBUTING.md), architecture
rule 3); a port does not reach around it.

## The X4 Pro, step by step

The X4 Pro has an ESP32-S3 with 8 MB of PSRAM where the X4 has an
ESP32-C3, a UC8179 display controller where the X4 has an SSD1677, touch,
a front light, a fuel gauge and a clock chip. It was added to the X4's
firmware rather than given one of its own, so the main loop, the
network, updates and remote control stay one. In the order it was done:

1. **A feature and a toolchain.** `x4-pro` in `shells/xteink/Cargo.toml`
   switches the esp crates to the ESP32-S3 (later shared with the X4
   Classic as `esp32s3`). The ESP32-S3 is Xtensa: it builds with espup's
   `esp` toolchain and `-Zbuild-std`, in the versions
   `shells/xteink/versions.env` pins (cargo aliases `x4pro`,
   `x4pro-run`, `x4pro-clippy`). `build.rs` links esp-hal's own memory
   map for it; the X4 has one of PaperPaw's (`memory-x4.x`), as the fonts
   need more flash than esp-hal maps on the ESP32-C3.
2. **The board.** `board_pro.rs` says which pin does what, with a table
   in its first comment, and takes the chip's peripherals once
   (`Board::take`). Everything else asks the board. Pins came from the
   device, checked against the community's notes (Papyrix, the FreeInk
   SDK); where sources disagree, the board says which was checked.
3. **The display.** `paperpaw_x4::uc8179` drives the controller, tested
   on a computer against a simulated controller: every command and where
   each band lands. The kernel draws in bands (`FRAME_ROWS`); with PSRAM
   the driver keeps the whole frame, so a fast refresh can compare. Four
   grays for pictures came after, from the controller's gray waveforms.
   Xteink ships the same device with different controllers, batch by
   batch: the firmware asks the panel which it has at start
   (`paperpaw_x4::probe`) and logs the answer.
4. **Input.** The X4 Pro's page buttons are on pins, read by
   `buttons::State::pins`. The X4's are on two resistor ladders read by
   the ADC, with `State::read`. Touch (`gt911`) is read in the same timer
   interrupt as the buttons, so a touch while PaperPaw is busy is not
   lost; its frame is mapped onto the screen by a `Transform`. The touch
   controller also reports the home key below the screen.
5. **The card.** The ESP32-S3 has an SD host (`card_pro.rs`), where the
   X4 uses SPI. Both mount the FAT file system the same way, in
   `volume.rs`.
   `paperpaw-fat` learned to read a partition that says it goes past the
   card's end, as the X4 Pro's cards come formatted.
6. **Power.** The battery's power latch is held through resets (a pad
   hold), so the device stays on while it is flashed and restarted.
   Sleep releases it on battery. Outer power is seen by the USB frames
   (`usb_power.rs`).
7. **Battery, clock and light.** The CW2017 fuel gauge and the BM8563
   clock chip share an I²C bus with the touch controller. The front light
   is PWM on two pins (`light.rs`), cool and warm.
8. **What the kernel is told.** `main.rs` gives the device's
   `DeviceInfo`: the id `xteink-x4-pro`, the name, 800 by 480 pixels at
   217 dpi, `PixelFormat::Mono1`, touch and its four keys, and
   `MemoryClass::Large`. It makes PSRAM the heap before anything
   allocates. It names the device for plugin libraries with
   `catalog_device("x4-pro")`, and says how the device started with
   `started`. Defaults that suit
   the device go here too (`setting_default`), never into a plugin.
9. **Updates.** `ota.rs` names the chip and the firmware that each
   update is checked against: `CHIP_ESP32_S3` and `paperpaw-xteink-pro`.
   The X4 Pro and the X4 Classic share their chip but not their board.
   So each has a firmware name of its own and refuses the other's
   firmware.
10. **Code plugins.** A plugin file runs only on firmware of its plugin
    interface and chip, built by the compiler the file names
    ([ADR 0026](adr/0026-plugin-interface-version.md)): the firmware
    exports its fingerprint (`PAPERPAW_NATIVE_FINGERPRINT`), and the
    release builds PaperPaw's store plugins for each device with it.
11. **The emulator.** A profile in `shells/desktop/src/profiles.rs` with
    the firmware's screen, buttons, memory budget and builder settings,
    so plugins can be tried for the device without it.
12. **CI and releases.** The device joins the matrices of `ci.yml` and
    `release.yml` (`device: [x4, x4-pro, x4-classic]`) and the
    `xteink-tools` action, which picks its toolchain, target and chip. An
    ESP32-S3 device also takes the `xtensa-std` action. A release then has its flash image, its signed update, its plugin files
    and its fingerprint.
13. **Fixes found on the device**, each a lesson for the next port (see
    the pitfalls below): the fuel gauge's battery profile written when
    missing, the gauge and the clock read from the main loop instead of
    the input interrupt, a stuck display ending in an error instead of a
    watchdog reset, card writes waited for by time, a finger's moves
    coalesced while the panel refreshes, the light in steps the eye sees
    as even.

The X4 Classic took a day after that. Its firmware comes with each
release as experimental: built, but not yet tried on one. It is an ESP32-S3 board with the X4
Pro's glass, card, gauge and clock, the panel on other pins, and the
X4's seven buttons each on a pin. It is a board file, `board_classic.rs`,
and a feature, `x4-classic`, with its id and firmware name, its emulator
profile and its place in CI. Everything else was the X4
Pro's. A port to a board like one PaperPaw knows is mostly a board file.

## Pitfalls

- **Watchdogs.** The RTC watchdog restarts the device after 20 s
  without the main loop (`WATCHDOG_S` in `main.rs`). Long work feeds it:
  every block the card reads or writes does. Waits on hardware end by
  time: the display gives up after 10 s, about three full refreshes. A
  wait without an end resets the device at every start.
- **The stack.** It lies above everything else in RAM and nothing stops
  it running over (`stack.rs` paints it at start and checks it). Deep
  recursion and large locals are the usual cause. The device check's
  Memory page and `x4-remote.py`'s `heap` say how much of it was never
  used; a frame reserved but not written (a stable sort's 4 KiB of
  scratch) does not show there, but does in `tools/stack-frames.py`,
  which reads the frames from the firmware's code.
- **The heap.** `MemoryClass::Small` devices have a heap of 262 KB, and
  opening a book takes up to 179 KB of it. Measure with the radio on:
  its buffers come from the same heap. With PSRAM, make it the heap
  first, so that the chip's own memory stays for the radio and DMA.
- **Interrupts.** Keep slow buses out of interrupts: an I²C read at
  100 kHz inside a critical section holds the radio's interrupts off.
  Read what can wait from the main loop.
- **Display sequences.** Use the stock firmware's sequences, as the
  community records them: weaker refreshes ghost on some panels, or do
  not take at all. Write both of the controller's memories after a
  refresh rather than trusting that it kept them. Ask the panel which
  controller it has; a driver for another shows nothing.
- **ADC curves.** Calibrate the ADC for its offset before reading button
  ladders or a battery's voltage: uncalibrated, the X4's readings were
  1600 too high and two buttons were lost. A fuel gauge needs its
  battery profile, which the stock firmware writes; after the battery
  was cut off, it reads zero without one.
- **USB and sleep.** While awake, idle light sleep waits for the USB
  link to be unused. Explicit sleep still enters hardware sleep on USB:
  a charging indication must not leave the processor or radio running
  behind a sleep image. Check power-button wake and USB enumeration
  again, with and without a cable. The log uses the chip's USB serial;
  esp-println's auto mode lost line ends on the ESP32-S3.
- **The stock firmware.** Save it before the first flash
  (`shells/xteink/README.md`), so the device can always go back. A first
  flash over it may need the power button held, as its boot loader does
  not keep the power on.
- **Toolchains.** Build with the versions `versions.env` and
  `rust-toolchain.toml` pin: plugin files run only on firmware of the
  same compiler, and newer Xtensa GCCs reject esp-hal's linker scripts.

## Checking a port on the device

Build the firmware with remote control over USB (the feature `remote`,
on in the cargo aliases), which also builds in the diagnostics plugin.
Then open **Tools → Device check** on the device. In a shell of another
kind, the action `diagnostics.open-check` opens it (in the emulator,
which runs scripts without its window:
`--headless --script run:diagnostics.open-check,shot:check.png`). It walks through
every part one page at a time, with buttons alone or by touch:

| Page | What it checks |
|---|---|
| Screen | Every gray level as a stripe; a pattern flipped with fast refreshes shows ghosting; a full refresh clears it |
| Buttons | Each button asked for by name; one that does not work is given up by holding another |
| Touch | Taps on targets in the corners and halfway along the edges |
| Front light | Brightness off, half and full, then cool and warm; the reader's own setting comes back |
| Battery | Charge, voltage, and charging seen on and off as the cable goes in and out |
| Card | 128 KiB written, read back and compared, with their speed |
| Clock | The time and date, to compare with another clock |
| Wi-Fi | Networks found, and whether one was joined |
| Sleep | Sleep, then wake by the power button; the check goes on after |
| Memory | The heap free and the stack never used since the start |

Each page ends with *Fail*, *Skip* and *Pass*, the answer its
measurements suggest standing out. Pages for parts a device lacks are
skipped, with the reason. After each answer the check writes its report
to `.paperpaw/device-check.txt` on the card, and it goes on where it was
when opened again.

`tools/x4-remote.py` takes screenshots and fetches the log and the
report. `--get .paperpaw/device-check.txt device-check.txt` copies the
report. `--script heap` says the heap's peak and the stack never used,
and `--help` lists all it does.

## The pull request

A pull request that adds a device is ready for review when it has:

- [ ] **Builds**: the firmware for the new device and for every device
      before it, each with and without `remote`
      (`cargo +"$FIRMWARE_TOOLCHAIN" build`, as `ci.yml` runs it).
- [ ] **Clippy for the target**, without warnings, with and without
      `remote` (`-- -D warnings`), and the workspace's checks from
      [CONTRIBUTING.md](../CONTRIBUTING.md): `cargo fmt`,
      `cargo clippy --workspace --all-targets`, `cargo test --workspace`
      and the `no_std` build for the X4's chip.
- [ ] **The firmware's size**: the image `espflash save-image` makes,
      within the app partition of `partitions.csv` (7.9 MB).
- [ ] **Memory numbers**: the heap's peak while a book opens with the
      radio on, and the stack never used (the device check's Memory
      page, `x4-remote.py`).
- [ ] **The device check's report**, the whole of
      `.paperpaw/device-check.txt`.
- [ ] **Photos of the Screen page**: one as it opens, to show the
      grays; one after flipping the pattern a few times, to show
      ghosting; and one after a full refresh.
- [ ] **The log line naming the display controller**, e.g.
      `display: UC8179 (FLG 13, REV 00 00 01 FF FF, BUSY high at rest),
      driven as UC8179`.
- [ ] **An emulator profile**, the CI and release matrices, and the
      documents brought up to date: a section in
      [ADR 0013](adr/0013-x4-firmware.md), `shells/xteink/README.md`
      and `CHANGELOG.md`.

In its description, say:

- which device and board, and where its pins and specifications come
  from (the device itself, a datasheet, a community project), marking
  what could not be checked;
- what was tried on the device and what was not, page by page of the
  device check;
- anything the device does differently from those PaperPaw knows (a
  controller, a bus, a button on another pin), and why the port does it
  that way;
- whether the kernel, the SDK or a plugin had to change, and why a
  capability or an SDK API was the way (see above).

The device's flash images then come with each release, and the web
flasher at [paperpaw.org/flash](https://paperpaw.org/flash) and the
plugin library offer the device once its id is added there; the
maintainers do that after the merge.
