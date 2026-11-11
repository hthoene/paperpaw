# Security model

Who can do what with a device running PaperPaw, and what protects the
reader's books, settings and passwords. It is short on purpose: a map of
the boundaries, so that changes can be judged against it. Vulnerabilities
are reported as [SECURITY.md](../SECURITY.md) says.

## What is protected

- **The firmware.** A device installs only firmware signed with
  PaperPaw's Ed25519 key, and it checks the signature while it writes
  the image. A new version that does not start cleanly is undone
  ([ADR 0019](adr/0019-firmware-updates.md)).
  The image paperpaw.org offers for flashing in the browser is signed
  too; the site and the browser check it before it is written.
  Firmware signed with another key (one's own builds) installs only
  when the reader accepts that key, shown by its fingerprint, at that
  install, and an older firmware only when the reader chooses it, told
  it is older: nothing on the card makes a key trusted.
- **Passwords and keys** (Wi-Fi, catalogs, sync accounts) are kept in
  `.paperpaw/secrets.txt`. **On the Xteink devices (X4, X3, X4 Pro, X4 Classic)
  they are encrypted** (decisions 138, 140 and 157): each value is sealed with
  AES-256-GCM under a key of that device, which stays in the chip's flash and never
  goes on the card. The authenticated data of each value is its owner, a zero byte
  and its name, so a value copied to another line does not open. The key and every
  12-byte nonce come from the chip's generator only while a radio is on (only then
  is it random); otherwise a save is refused (`ERR_STATE`, the screens ask to turn
  Wi-Fi on) and nothing is written, and a nonce that repeats the previous one of
  the start, or is all zeros or ones, is refused too (`ERR_IO`: the generator is
  stuck, which a radio does not mend). Where the key's sector holds something that
  is not ours (another firmware's data) or the table gives it no place, nothing is
  sealed and a save says to reinstall once with the web flasher (`ERR_UNSUPPORTED`);
  the sector is never written. There is no veiling for a new
  value. Values written by Beta 3 and earlier (interface 6) are **veiled, not
  encrypted**, so that they do not show at a glance: whoever has the card has
  those, until the first start of this version that has a radio on seals them all
  (at start, or the first time Wi-Fi or Bluetooth comes on; tried again only after
  weak numbers, at most once each time a radio comes on). A card in another
  device opens none of the values: they count as absent, the user is told once per
  start that saved passwords from another device cannot be opened (not for a flash
  that is only busy or unreadable then: those values are absent for the moment), and they are
  entered again; the lines stay for the device they belong to. The key is one 4 KiB
  sector (`paperpaw_x4::device_key`):

  | Device | Key sector | Where it lies |
  | --- | --- | --- |
  | X4, X3, X4 Pro | `[0xC000, 0xD000)` | the fourth sector of the `nvs` partition of `partitions.csv` (`0x9000`, 16 KiB) |
  | X4 Classic | `[0xFFF000, 0x1000000)` | the last sector of the 16 MB flash, in no partition of `partitions-classic.csv` |

  The Classic's `nvs` (`0x9000` to `0xE000`) is the factory's, kept whole and never
  written (it says which panel the device has), so its key is not a page of it. A
  partition table that has a partition over the Classic's sector (CrossPoint's
  `coredump` at `0xFF0000`) makes the firmware seal nothing there: a save is
  refused with `ERR_UNSUPPORTED` and a hint to reinstall once with the web flasher
  (decision 141); values already saved stay readable. A key sector that holds
  something that is not PaperPaw's record (a page of another firmware's NVS) is
  never erased or written: nothing is sealed then. **A full-image flash or an
  erase of the whole flash erases the key**, and then the sealed values cannot be
  read any more. The web flasher skips the sector, so a reinstall keeps the
  passwords (decision 139; `paperpaw_x4::device_key::kept_range`, pinned by a test
  against the shipped tables). **Another firmware on the same flash** (Xteink's
  stock firmware or CrossPoint in a dual boot, both with an ESP-IDF NVS over
  `0x9000` to `0xE000`) may erase the X4, X3 and X4 Pro sector, because to its NVS
  the key is a page it does not know. Nothing is exposed then: the values count as
  absent, the user is told and enters them again, and the next save makes a new
  key. Whether the stock firmware does erase it is to be checked on a device (key
  sector read back before and after, #250).
  The desktop emulator seals with a key file of its own user's (`shells/desktop`,
  `docs/abi/secrets.md`). On Android the file is in the app's private folder,
  which only PaperPaw reads. They are never shown
  in full; screens show dots and ask for them afresh. Wi-Fi networks the
  web flasher gives with the firmware (X4s) go to the card as `wifi.ini`,
  are kept with the passwords at the next start and the file removed;
  the flash keeps them only until the card has them.
- **Books and notes** are files on the card, as readable as the card is.

## Who can reach the device

- **Physical access (the card, USB).** Whoever holds the device can read
  and change the card and flash any firmware over USB (the chip's own
  bootloader); PaperPaw does not try to stop that. Firmware built for
  testing has a remote control over USB: files in and out, presses, the
  screen. Released firmware is built without it (the `remote` feature
  of `shells/xteink`).
- **The cable transfer.** Every firmware takes plugins and books from a
  computer over the USB cable ([docs/usb-transfer.md](usb-transfer.md)).
  It writes nothing before the reader allows it on the device: the
  device shows what is to come, with names and sizes. It takes only what
  it showed, each file by its size and SHA-256, and never writes over a
  book. An answer counts only for the offer shown, and *Back* ends the
  transfer. The Wi-Fi transfer installs plugins the same way.
- **The local network.** The device listens only while a screen that
  serves a page is open (the Wi-Fi transfer, typing on a phone, the
  flash cards' *Edit with your phone*, setting up Wi-Fi with a phone).
  Nothing listens in the background. The transfer, typing on a phone
  and the flash cards show an address as text and as a QR code. The
  address carries a key of 65 bits, drawn anew each time (`?k=`), and
  every request must carry it. After five wrong keys, no request is
  answered for a minute (`paperpaw_sdk::served`).
- **The reader's own network.** Where PaperPaw runs Wi-Fi itself, as on
  the X4s, two screens open a network of the device's own: the Wi-Fi
  transfer without a router and the setup of Wi-Fi with a phone. The
  network uses WPA2 with a password drawn anew each time. The password
  shows only on the screen, also as a QR code to join.
  The Wi-Fi setup serves its page only there and takes only the form it
  gave out. The network closes with the screen, or after ten minutes
  in which nothing comes.
- **The internet.** The device only connects out, for catalogs, sync,
  updates and news. Where the address says `https`, it uses TLS and
  checks the certificates against the clock, which it sets over the
  network first.
  An update search sends the chip, the version, the plugin crates built
  in, the fingerprint code plugins need and the code plugins installed
  with their versions, no identifier: when the
  reader asks, once a week where the reader lets it look by itself, and
  once a day while an update found waits for plugins
  ([ADR 0022](adr/0022-plugins-in-updates.md)). To be counted without
  being known, it also says whether the reader asked, and whether it is
  the device's first search of the day, the week or the month (UTC): the
  device keeps the date of its last search for that, and never sends it. The store asks for its
  list with the kind of device and the language, no identifier either.
- **Files.** Books, fonts, pictures and catalogs are untrusted input.
  The parsers bound their memory and time, and they reject what they
  cannot read. CI fuzzes six of them: EPUB, text, XML, hyphenation,
  images and Lua.

## Plugins

- **Code plugins** run as native code without a sandbox, with the
  kernel's rights. Through the SDK, a plugin reaches all of the card,
  the network and its own passwords and keys. Past the SDK, its code can
  reach everything, other plugins' passwords too. They are built into
  the firmware or the app. On the X4s they also come as plugin files
  (`code.ppn`, [ADR 0024](adr/0024-native-code-plugins.md)): from the
  store, with a firmware update, from the web flasher, or from a
  computer over the cable or the Wi-Fi transfer. A published file is
  signed by its author (Ed25519, `paperpaw-sign`); PaperPaw's own plugin
  files are signed with the project's plugin key. The store and a
  firmware update install a file only when it is made for this firmware
  and signed by the key the plugin library names for its author; the device remembers
  that key at the first install. An update signed by another key is
  refused, in the store, in a firmware update (which does not fetch it
  at all) and from the web flasher, with no way for the reader to accept
  the new key; the version installed stays. The one way to another key
  is the author's **recovery key**, chosen in advance and pinned with the
  author's key at the first install, never later
  (`.paperpaw/plugin-recovery.txt`, `native::keys::remember_first_install`):
  a statement it signs (`paperpaw-sign rotation`) moves the plugin to a
  new key, and the device checks it itself
  (`native::keys::accept_rotation`), so neither a moderator nor a
  taken-over server can forge one. The code installed before the move is
  no longer named as installed: it may be what the stolen key signed, so
  only an update by the new key runs. Install marks never move a known
  author to another key, and name code as installed only where the
  author pinned then signed it. A **revocation** (a key stolen;
  `.paperpaw/revoked-keys.txt`, `native::keys::keep_revoked`) is a
  statement about one plugin and the key pinned as its author there,
  signed by that key itself or by the plugin's recovery key
  (`paperpaw-sign revocation`): the store only carries it, so neither the
  library nor a taken-over server can revoke a key it does not hold. It
  stops that plugin's code (the kernel does not load it, `Revoked`), not
  other plugins the same key signed, which have revocations of their own.
  One once kept stays, whatever a later list says; a line that does not
  hold (unsigned, forged, about a key not pinned) counts for nothing, and
  the kernel checks every line again where it reads the list (it came
  from the library). These signatures guard against outside attackers (a
  rogue or compromised library, the network, forged or replayed
  statements), not against plugins: installed plugins are trusted with
  the whole card (owner decision 44, no sandbox), the author, recovery
  and revocation lists in `.paperpaw/` too (see *What is not protected*). From the web flasher and from a
  computer the firmware also takes a file not signed (one's own build):
  from the flasher only for a plugin that has no author's key remembered
  yet, from a computer only once the reader said yes to its code on the
  device. Only code installed so runs: a file put into a plugin's folder
  by hand is not loaded. The list of code installed
  (`.paperpaw/plugin-code.txt`) is on the card, so whoever holds the card
  can change it, as test firmware does over USB; the transfers and
  plugins' own folders do not reach it. Code plugins are trusted as
  their author is.
- **Data plugins** (folders with `plugin.ini`) carry no code: translations,
  key maps, defaults of settings, presets, menus, actions made of steps,
  marks in the text, icons, steps of the first-run setup, lines for
  other plugins' points, fonts and stylesheets. They can change what
  PaperPaw shows and does through those, not reach past them, and never
  set a setting that keeps the device safe (such as the lock's).

## The screen lock

The lock is a PIN, a pattern or a password. It is asked for at start
and, unless the reader turns this off, on waking. It keeps a person who
picks up the device from using it: from reading what is open, changing
settings or seeing passwords on screen.
It is a convenience, not encryption: the card is as readable as without
it, and whoever takes the card out or flashes firmware gets past it. The
code is kept as a salted hash, never as it is.

Plugins are trusted (above), so the lock does not guard against them.
What the SDK makes sure, for plugins used honestly:

- **Codes are the lock's alone.** While the lock guards the device, what
  its screen or the keyboard it opened gives (the password typed) goes
  to the lock and that keyboard only: no other plugin's subscription,
  screen, bar or rule hears it. The kept code is a secret no other
  plugin is given.
- **Its settings are its own.** Settings marked guarded (the lock's kind
  and when it locks) are changed only by their plugin and the settings
  editor: not by presets, power modes, data plugins' defaults or other
  plugins' commands.
- **Nothing gets past it.** Screens opened while it guards go below it;
  the actions, notices and bars of other plugins wait; only the lock's
  own input filters see input. A screen of another plugin left above it
  (a sleep screen) gets no input and closes at the next input or as the
  device wakes, so the reader is never locked out.

The slots stay replaceable: a keyboard or a settings app put in place of
PaperPaw's is a trusted customisation and sees what the original would.

## What is not protected

- A lost or stolen X4 **with** its card: the device key is in the chip's
  flash, which is not encrypted, so whoever holds the device can read the key
  over USB and open the values. The card alone opens nothing. Beta 3's veiled
  values until they are sealed (see above).
- Firmware flashed over USB by someone holding the device.
- What a code plugin does with its rights, built in or installed: it can
  also change the lists of authors, recovery keys, revocations and code
  installed in `.paperpaw/`.
