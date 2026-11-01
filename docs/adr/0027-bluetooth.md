# 0027. Bluetooth Low Energy

- Status: Accepted; steps 1 to 3 built (the X4 Pro as central: scan, pair,
  bonded remotes and keyboards, HID keys), step 4 (advertising, the GATT
  server) not yet: the shell drops it at once. The Pro's current and
  light sleep with the radio on are not measured yet (ticket #2).
- Date: 2026-10-24

> **Measured 2026-11-01 (firmware engineer, from the build, not a device):**
> the X4 Pro image of `iface5` is 6,244,144 bytes with Bluetooth and
> 5,787,280 without (the same commit with `x4-pro = ["esp32s3"]`): **Bluetooth
> costs 456,864 bytes of flash (446 KiB)**, not the 75 KB of the spike
> (trouble-host, the controller's library, coexistence, logging). `.text`
> grows by 375,460 bytes, `.rodata` by 52,200. The heap is **26 KB smaller
> for good** (`HEAP_SIZE`, `BLUETOOTH_KB`), also with the radio off. This is
> most of why the smallest slot margin of the Pro is 302 KiB (CI fails below
> 256 KiB). Notes: `investigations/2026-11-01-bluetooth-pro.md`.

## Context

Readers ask for Bluetooth page-turners: a small remote, or a keyboard,
that turns pages while the device rests on a stand. Plugin authors want
more: heart-rate belts and other sensors for reading statistics, a
phone companion that finds the reader without Wi-Fi, a foot pedal, a
smart light. A spike on the X4 Pro (branch `ble-spike`, bc7aea04) found:

- **It runs.** esp-radio's BLE controller with `coex` beside Wi-Fi, up
  in 4 ms, scanning 53 ms after the command; Wi-Fi joined during a scan.
  esp-radio gives an HCI connector (`BleConnector`) that implements
  bt-hci's `Transport`, so trouble-host (Embassy's host stack: central
  and peripheral, GATT client and server, LE Secure Connections) plugs
  in as it is.
- **It costs** about 75 KB of flash, 21.6 KB of IRAM for the
  controller's code, and 35.4 KB of internal RAM while the controller
  is up, all of it back when it is down.
- **A trap**: on the ESP32-S3, IRAM and DRAM are one SRAM, so the
  controller's code shrank the main stack from 43.4 KB to 19.8 KB and the
  firmware hit its stack guard at start. The spike guessed a smaller
  internal heap (160 KB instead of 192 KB). A product needs a budget
  that a build checks.
- **Battery is not measured.** Receiving costs some 80 to 95 mA at full
  duty, and the main loop does not light-sleep while a radio is on. So
  Bluetooth, like Wi-Fi ([ADR 0016](0016-network-and-wifi.md)), must be
  off unless something needs it, and its work must fit declared work
  ([ADR 0025](0025-power-saving.md)).

The forces are those of Wi-Fi and sockets
([ADR 0014](0014-sockets-for-plugins.md)): the kernel is sans-I/O
([ADR 0003](0003-sans-io-kernel.md)), the radio is shared by plugins
that must not see each other's devices or keep it on for nothing,
everything must be replaceable ([ADR 0004](0004-everything-is-a-plugin.md)),
and code plugins are bound to an interface version
([ADR 0026](0026-plugin-interface-version.md)).

## Decision

Bluetooth Low Energy (not Classic) becomes a capability of the kernel,
in the manner of Wi-Fi: **the mechanism in the shells, arbitration and
safety in the kernel, policy in a first-party plugin**, and a generic
vocabulary in the SDK that is not about page-turners. Page-turners come
for free on top: a bonded keyboard's keys are inputs of the key maps.

### 1. The SDK: `paperpaw_sdk::bluetooth`

A new module. Every type that may grow is `#[non_exhaustive]` with a
constructor (rule 1), errors are typed (rule 2), and builders take
`impl Into` (rule 7).

- **Who may**: `Uses::BLUETOOTH` in the manifest, shown in the store
  and the plugin manager like `Uses::NETWORK`. The kernel refuses every
  Bluetooth command of a plugin without it (`BtError::NotAllowed`).
- **Where**: `Capabilities::bluetooth` (the device has a radio the
  kernel may use), `MenuNeed::Bluetooth` for settings and menu entries.
  `Env::bluetooth()` is `Some` only there, a `BluetoothView` with what
  the plugin may know: the radio's state (`Off`, `On`), whether
  something uses it, the most links the device keeps (`max_links`), and
  the names and kinds of the bonded devices given to it (no addresses).
  The Bluetooth manager (the plugin whose screen is bound to
  `slots::BLUETOOTH`) sees every bond and whom it is given to.
- **Values**:
  - `Uuid`: 128 bits, with `Uuid::sig(0x180D)` for the Bluetooth SIG's
    16-bit numbers and `Uuid::parse`.
  - `DeviceId`: an opaque number the kernel gives a device. It is
    stable across starts for a bonded device; plugins never see
    addresses, which may be random and change.
  - `Device`: id, name, `signal`, the services and the manufacturer's
    and services' data it advertises, its transmit power and
    appearance, `bonded`, `DeviceKind` (`Keyboard`, `Remote`, `Sensor`,
    `Phone`, `Other`, from its appearance and services).
  - `GattService`, `GattCharacteristic` (uuid, a `Handle`, `Properties`:
    read, write, write without response, notify, indicate), with their
    descriptors.
  - Values are bytes (`Vec<u8>`), carried in events as typed values
    (`Params::carrying`), as `HttpReply`'s body is.
- **Commands**: `Command::Bluetooth(Box<BtRequest>)`, the kernel
  stamping the sender, written with builders:

  ```rust
  bluetooth::scan(&FOUND)            // results come as FOUND events
      .service(Uuid::sig(0x180D))    // filters: services, name prefix,
                                     // manufacturer
      .lasting(Duration::from_secs(10))  // a scan always ends
      .command()
  bluetooth::connect(device, &LINK)  // all about the link: LINK events
      .power(LinkPower::Low)         // the default
      .idle(Duration::from_secs(60)) // let go when nothing was asked
      .command()
  bluetooth::discover(device).command()  // services, characteristics
  bluetooth::subscribe_to(device, service, characteristic)
  bluetooth::read(device, handle)
  bluetooth::write(device, handle, bytes).without_response()
  bluetooth::subscribe(device, handle)    // notifications or indications
  bluetooth::unsubscribe(device, handle)
  bluetooth::pair(device)            // asks the user, see 3.
  bluetooth::disconnect(device)
  bluetooth::advertise(&SEEN)        // be seen, see below
      .name("PaperPaw")
      .service(MY_SERVICE)
      .lasting(Duration::from_secs(60))
      .command()
  bluetooth::notify_centrals(service, characteristic, bytes)
  bluetooth::respond(request, bytes) // to a central's read or write
  bluetooth::refuse(request, AttError::READ_NOT_PERMITTED)
  bluetooth::remote_keys(device, &DONE)  // a remote's keys, see 2.
  bluetooth::forget(device, &DONE)   // the reader's or the manager's
  ```

- **Events**: one event of the plugin's per request or link
  (`scan(&FOUND)`, `connect(device, &LINK)`), as sockets have, each a
  `BtEvent`: `Found(Device)`, `ScanEnded`, `Connected { device, mtu,
  security }`, `Disconnected { device, reason }`, `Discovered { device,
  services }`, `ServicesChanged`, `Value { device, handle, kind, bytes }`
  (a read, a notification or an indication), `Written`, `Sent`,
  `Paired`, `Done { op, device }`, `Failed(BtFailure)`, and the GATT
  server's and advertising's events; `Extended { key, params }` leaves
  room for more without raising the interface. `BtFailure` says which
  command failed (`BtOp`), for which device and handle, and why
  (`BtError`: `Off`, `NotAllowed`, `NotFound`, `Busy` (no link left),
  `Timeout`, `Declined` (by the reader), `Lost`, `Att(AttError)` (the
  device's GATT error), `Held(Refusal)` (not now, with when to try
  again), `Unsupported`). Each command a plugin sends is answered, as
  declared work is: nothing is refused silently.
- **A GATT server**: `Registrar::add_gatt_service(ServiceSpec)`,
  declared at registration with its characteristics and their
  properties, so the kernel builds one attribute table for all plugins.
  A central's reads, writes and subscriptions come as the plugin's
  events (`BtEvent::ReadRequest`, `WriteRequest`, `Subscribed`); it
  answers with `respond` or `refuse` and sends with `notify_centrals`.
  A characteristic needs an encrypted link unless its `Access` says
  otherwise. Advertising is asked
  for with a duration and ends with it or with the screen that asked,
  whichever is first; the kernel merges what several plugins ask into
  one advertisement.
- **Background work**: `Work::bluetooth(id)`, its `needs` holding
  `Needs::BLUETOOTH` beside `Needs::NETWORK` and `Needs::CHARGING`, for
  work that needs the radio (reading a sensor every hour), done at
  cheap moments like the network's ([ADR 0025](0025-power-saving.md)),
  with `Moments::BLUETOOTH_ON` and `Reason::BluetoothOff`.
- **Energy**: see 8.

### 2. Input: keyboards and page-turners

- A bonded device that offers the HID service, and that the user let
  type when pairing (the default for keyboards and remotes), is the
  kernel's: it subscribes to its reports and turns them into button
  events, `InputEvent::Button { name: "hid.right", .. }` with press and
  release, so the key maps, long presses and rules see them like the
  device's buttons. No plugin parses HID for that.
- The names are a fixed table of the keyboard and consumer usages that
  matter (`hid.right`, `hid.left`, `hid.up`, `hid.down`, `hid.page-down`,
  `hid.page-up`, `hid.space`, `hid.enter`, `hid.escape`, `hid.volume-up`,
  `hid.volume-down`, `hid.play-pause`, `hid.next-track`,
  `hid.previous-track`, the letters and digits), in
  `paperpaw_platform::buttons`. The default key map turns pages with
  right, page down, space, volume up and next track (and back with their
  opposites), which covers the cheap remotes (most are HID keyboards
  sending arrows, page keys or volume). The keys plugin lists them under
  a Bluetooth heading where a remote is bonded.
- Decoding is a crate without I/O (`paperpaw-hid`): the boot keyboard
  report and consumer control reports by the device's report map,
  tested on recorded reports. The shell hands raw reports to the kernel,
  which decodes, so every shell gets the same keys.
- A plugin that wants the raw reports (a game pad, a pedal with its own
  report) asks with `bluetooth::hid_reports(device, &REPORTS)` for a
  device granted to it (3.); the user decides on the device's page
  whether its keys still go to the key maps.

### 3. Permissions and safety

- **Pairing is the user's.** A plugin may ask (`pair`), but the kernel
  asks the user in its own prompt (a core screen, not the plugin's):
  the device's name, and the code to compare or type where the device
  wants one (numeric comparison, passkey display or entry). Just Works
  pairing still needs a confirmation. A refusal is `BtError::Declined`.
- **A plugin sees only its devices**: those its own scans found, and
  bonded devices the user gave it (on the device's page in the
  Bluetooth settings, "Used by"; asked once when a plugin pairs one).
  Commands about other devices fail with `NotFound`, as if they were not
  there. Scans of several plugins are one scan for the radio, each
  plugin getting only the results that pass its filters.
- **Bonds are secrets.** The keys a bond makes (LTK, IRK) are kept with
  the passwords and keys as `bluetooth.<device>`, under `.paperpaw` like
  them (`core.bluetooth.<n>`), and the user sees and removes them there
  or on the Bluetooth page. The kernel hands them to the shell at start and takes new ones
  from it. Forgetting a device removes its keys and its grants.
- **Links end**: a plugin's scans, advertising and links end with the
  screen that was showing when it asked (as sockets do), unless they
  were asked for by declared work, which has its own limits; all end
  before the device sleeps. The kernel's HID links follow the policy
  plugin (5.).

### 4. The kernel and the shells

- **The kernel** (built with `KernelBuilder::bluetooth`) keeps each
  plugin's scans, links, subscriptions and advertising, numbers them for
  the shell (`Io::Bluetooth`, `Kernel::take_io`) and turns the shell's
  reports (`Kernel::bluetooth_event`) into the plugins' events. It
  switches the radio on when the first request comes and off when none
  is left (after a short grace for the next one), limits links to
  `max_links`, and emits `core.bluetooth-changed`. Kernel tests cover
  all of it with made-up shell reports.
- **The X4 Pro** (feature `bluetooth`, included by `x4-pro`):
  trouble-host on esp-radio's `BleConnector`, polled by the
  main loop as the radio's futures are. Its memory is a budget, not a
  guess:
  - static: the controller's code in IRAM (20.6 KB measured) and its
    statics (4.4 KB), taken from the internal heap at build time (166 KB
    instead of 192 KB with the feature: the main stack keeps its 41.3
    KB), with a check in the firmware's build (a linker assertion, 40 KB
    at least) that the main stack keeps its size, so the spike's boot
    loop cannot come back unseen. Nothing else stays while Bluetooth is
    off: the host, its packets and resources are on the heap only while
    the radio is up;
  - while up: the controller's 35.4 KB of internal RAM (measured), and
    the host's pools sized for two links (a remote and one more), an ATT
    MTU of 247 and the discovered services, put in PSRAM where the host
    allows; the total is measured and written down before the feature
    is on by default;
  - with Wi-Fi on at the same time: allowed, measured with both up; if
    the internal RAM left is below the firmware's floor, the kernel
    refuses the second radio's start with `BtError::Busy` rather than
    run out.
  - Light sleep with the radio on is a requirement for the default
    "while reading" policy; until it works, the policy's default is off,
    and the current is measured with a real remote before it changes.
  - trouble-host is used as `patches/trouble-host` (0.7.0, "Modified for
    PaperPaw"): it stopped the whole host where the controller refused
    to switch address resolution on again after syncing its resolving
    list, because a scan or connection had begun meanwhile. The copy
    tries the sync again later. Once a trouble-host release retries or
    ignores that refusal itself, the copy and its `[patch.crates-io]`
    in the firmware go, and so may the firmware's short waits before it
    scans again (after giving the host its bonds, after a connection).
- **The emulator** pretends, for the X4 Pro profile and on request for
  others, with made-up devices so authors and the Workshop can try
  plugins without hardware: a page-turner (an HID keyboard sending right
  and left, pressed from the emulator's window or a script step,
  `ble-key:right`) and a heart-rate sensor (service 0x180D, a
  notification each second), more as files describing a device's
  services and values. Pairing goes through the same prompt.
- **Android**, later: its BLE API (scan, `BluetoothGatt`,
  advertising), with its runtime permissions asked when a plugin with
  `Uses::BLUETOOTH` first needs them. Keyboards paired with Android
  already send key events, which the shell passes on as `hid.*`.
- **The web shell**: none (`Capabilities::bluetooth` false).
- **The plain X4 (ESP32-C3)**: not now. It has BLE, but 35 KB of
  internal RAM and 21 KB of IRAM are a large share of what reading and
  Wi-Fi leave there, and it has no PSRAM. The SDK does not depend on it:
  once the Pro's numbers are measured, a page-turner-only mode (radio
  exclusive with Wi-Fi, one link) may be tried on the X4, and adopted
  only if reading keeps its memory.

### 5. First-party plugins

- **`bluetooth`**: the policy and the settings page, modelled on
  Wi-Fi's. Its page: the radio on or off, the bonded devices (name,
  kind, connected, "Used by", forget), and "Add a device" (a scan
  showing what is found, with the strongest first, and pairing). Its
  setting "Remote" says when bonded keyboards and remotes are
  connected: while reading (a book open), while awake, or only by hand;
  reconnecting is declared work, so it happens at cheap moments and not
  in a loop. In the Saving power mode it connects only by hand.
- **Page-turners** need nothing more: the default key map (2.) and the
  keys plugin's page, where the reader rebinds `hid.*` like any button.

### 6. The interface

The typed design adds a variant to `Command` and `MenuNeed`, a field to
`Capabilities` and `Usage`, `Work`'s `network` and `charging` become one
`needs`, and a way from `Env` to the state: changes of
types that cross, so the plugin interface is raised once (2 to 3).
Everything else (the module and its types, `Uses::BLUETOOTH`, the
`hid.*` names) is new and free. The raise lands with the first step
and goes into the next release with any other raise waiting, so authors
rebuild once.

An additive route was weighed and not taken: requests as events the
kernel listens to (`Command::Emit` with reserved ids, values as text
parameters) and the state as a service of the core plugin. It changes
no type, but it is stringly typed, invisible to the compiler and to the
API files, and turns `Emit`, a broadcast, into a request channel.

### 7. Rollout

Each step lands green on its own:

1. **SDK and kernel**: the module, `Uses::BLUETOOTH`, the commands and
   events, the kernel's arbitration, grants and pairing prompt,
   `paperpaw-hid` with the `hid.*` inputs, the interface raised. The
   emulator's made-up devices. Tests in the kernel and the SDK; the
   example plugin below built and run in the emulator.
2. **The `bluetooth` plugin**: the page, pairing, forgetting, grants,
   the "Remote" policy and the default key map, in the emulator (a
   page-turner turns pages, a heart-rate belt shows in the example).
3. **The X4 Pro**: trouble-host behind `bluetooth`, the memory budget
   and its build check, the HID host on a real remote, the current
   measured; then the capability on.
4. **Being seen**: advertising and the GATT server, in the emulator,
   then on the Pro, with a phone's BLE scanner as the other side.
5. **Android**, then the X4 decision (4.).

`docs/plugins.md` gains a section "Bluetooth" (the permission, scanning
and connecting, reading and subscribing, being seen, keys of a remote,
testing with the emulator's devices), and the skill for community
plugins learns it once step 1 is released.

### 8. Energy

Bluetooth must cost no more than Wi-Fi does, so every default is the
cheap one and the kernel never spends more than someone asked for:

- **The radio is off** unless a scan, a link or advertising needs it,
  and goes off three seconds after the last; it has no wake-ups of its
  own, and everything ends before the device sleeps.
- **Scans end**: 10 seconds by default, a minute at most, passive and
  seldom (`ScanPower::Low`). `ScanPower::Fast` only as the reader's own
  doing and for 5 seconds at most; a plugin scans again only a minute
  after its last scan, unless the reader asked (else
  `BtError::Held`). Longer scans belong to declared work.
- **Links are slow**: long intervals with peripheral latency
  (`LinkPower::Low`) unless asked; `.idle(...)` lets a link go when
  nothing was asked for a while. A remote's link is responsive while
  keys come and slows down a minute after the last.
- **Advertising ends**: a minute by default, ten at most, `Fast` only as
  the reader's own doing and for 30 seconds.
- **The cheapest that serves all**: where plugins ask for different
  powers, the scan, each link and the advertisement run at the highest
  power someone asks for right now, and drop again when they stop.
- **Counted**: the time the radio was on goes into
  `Usage::bluetooth_ms`, for the battery's statistics.
- **Reconnecting remotes**: while a plugin wants a bonded device's keys
  (the Remotes setting) and its link is gone (a remote falls asleep), the
  shell watches for it: a passive scan of 30 ms every 1.28 s (2.3 % of
  the time), only the bonded devices' advertisements told, and the
  kernel connects one as soon as it advertises (on the X4 Pro a keyboard
  woken by a key was connected within a second or two). Never when the
  keys are not wanted, never in sleep. The radio stays on meanwhile, and
  as long as the main loop cannot light-sleep with it on (§4) that is
  what it costs; measured before "While reading" is the default.
- **Beside Wi-Fi**: the S3 has one radio for both. With Bluetooth on,
  Wi-Fi's modem always sleeps between beacons, and links run at the slow
  intervals while Wi-Fi joins or moves data, with supervision timeouts of
  8 s. On the X4 Pro a keyboard's link then held through a Wi-Fi join, a
  search and downloads (downloads took 1.4 s instead of 0.3 s), and keys
  kept coming.

The X4 Pro measures a bonded remote's current, idle and while keys
come, and light sleep with the radio on works before the Remotes
"while reading" default is switched on (7., step 3).

### The example a plugin author writes

A heart-rate reader: it scans for the standard heart-rate service,
connects to the first found, subscribes to the measurement and shows
the beats per minute.

```rust
const FOUND: Event<BtEvent> = Event::new("pulse.found");
const LINK: Event<BtEvent> = Event::new("pulse.link");
const HEART_RATE: Uuid = Uuid::sig(0x180D);
const MEASUREMENT: Uuid = Uuid::sig(0x2A37);

fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_action(ActionSpec::new(START, "pulse.start").handler(|_, _, out| {
        out.push(bluetooth::scan(&FOUND).service(HEART_RATE).first().command());
    }))?;
    r.on(FOUND, |_, event, out| {
        if let BtEvent::Found(device) = event {
            out.push(bluetooth::connect(device.id, &LINK).command());
        }
    })?;
    r.on(LINK, |env, event, out| match event {
        BtEvent::Connected { device, .. } => {
            out.push(bluetooth::subscribe_to(*device, HEART_RATE, MEASUREMENT));
        }
        BtEvent::Value { bytes, .. } => {
            // Flags first; the rate is a byte, or two where flag 0 says so.
            let bpm = if bytes[0] & 1 == 0 { u16::from(bytes[1]) }
                      else { u16::from_le_bytes([bytes[1], bytes[2]]) };
            out.push(Command::set(&BPM, i64::from(bpm)));
        }
        BtEvent::Failed(_) => out.push(Command::Notify(env.i18n().tr("pulse.lost").into())),
        _ => {}
    })
}
```

with `Uses::BLUETOOTH` in its manifest. In the emulator the made-up
heart-rate sensor answers it; on the X4 Pro a real belt does.

## Consequences

- Plugins get Bluetooth LE in general (scanning, GATT client and server,
  pairing through the user), testable without hardware, and the radio
  stays off unless a plugin or the reader needs it.
- Page-turners and keyboards work through the key maps with no plugin
  code, and the reader rebinds their keys like the device's buttons.
- Devices and bonds are private: a plugin sees what it found or was
  given, and pairing is always confirmed on the device.
- The plugin interface is raised once; code plugins are rebuilt with the
  release that brings it.
- The X4 Pro pays about 390 KB of flash with the host stack (measured:
  trouble-host 126 KB, the controller's libraries 83 KB, the shell's
  Bluetooth code 66 KB, the rest pairing's cryptography and the kernel's
  part; the spike's raw HCI took 75 KB) and 26 KB of internal heap
  for good. While the radio is up the controller takes about 35 KB of
  internal RAM and the host some 18 KB of heap (PSRAM): its state 10 KB,
  its resources 4 KB, packets up to 4 KB. The firmware's build checks
  the stack budget that the spike broke. In the emulator a connected remote adds nothing measurable to
  reading's peak (a test keeps it under 8 KiB). The plain X4 waits for
  measurements.
- Battery is the open risk: the default policy stays manual until light
  sleep with the radio on works and a remote's current is measured.
