# Picture frame on interface 7

Status: **complete.** A new plugin made for interface 7 (draft 36, no interface change); there is no
interface-6 part. Version 1.0.0. A store plugin (`tools/plugin-files/plugins.txt`, format-5 file by
`tools/plugin-files7`), not linked into the firmware.

## What it does

- **Photos:** a folder of the card (setting `picture-frame.folder`, `/photos` at first; JPEG and PNG, the
  first 512 by name within 24 KiB of names, as the sleep plugin keeps a folder) and the cache of a WebDAV
  server (below). The list is read at the start, when the folder changes and after a fetch.
- **The sleep screen** (`picture-frame.sleep`): offered to the slot `core.sleep` below the sleep plugin's own
  screen (priority -1) with a `setting_choice` of `sleep.show`, as `reading-goal.sleep` and
  `vocabulary.sleep-card` are; the sleep plugin binds the slot when the reader chooses it. The photo is
  chosen **at each sleep** (the one after the last, `cursor` in `state.txt`, advanced when the sleep screen
  closes) or **daily** (`day mod count`, days since 1970 by the local time), by the setting `change`. The
  next photo is asked for with `image_prepare` while the device is awake (at the start, when the screen
  closes, when a setting changes), so the first draw of the sleep screen has it: the host cannot decode
  while the device falls asleep. Drawn over white with `ImageStyle::grays()` (the panel's grays at once,
  Decision 216) and fitted by the setting `fit` (see below).
- **The slideshow** (`picture-frame.show`, *Slideshow* in the tools, one step; or from the home screen): the
  next photo after a minute tick that makes `interval` minutes **while power is plugged in**
  (`ChargeChanged` external power or charging); without power by the buttons (`core.left`/`core.right`,
  *Select*), a swipe or a tap on the left third or the rest. `slideshow-on-charge` opens it when the charger is
  plugged in. A new photo is drawn with a full refresh (it is final, in grays).
- **The widget** *Photo of the day* (`picture-frame.widget`, 2 by 2 cells, the shared look settings): the
  photo `day mod count`, asked for when the widget is made and at a new day (the minute tick); a tap opens
  the slideshow.
- **Greyscale for e-ink:** the host decodes (`image_prepare`: to the screen's size, grey, EXIF turn applied,
  PNG and JPEG up to 8 megapixels; the host reads and draws in bands, so no plugin ever holds a photo) with
  a **tone curve** this plugin makes from the `contrast` setting (-50 to 50 %, 10 at first) and draws with
  `ImageStyle::grays()` (the host's dither of the panel). **Fit:** *Automatic* fills (crops, `ImageStyle::fill`)
  where the photo's shape is within 4:3 of the screen's and shows the whole photo (centred, white edge)
  where it is not, so a landscape photo on the portrait screen is not cut to a stripe and the same photo
  in landscape fills it; *Whole photo* and *Fill the screen* are the two extremes. Portrait and landscape
  follow the screen's orientation (the draw rectangle).

## The server (WebDAV)

`PROPFIND` depth 1 of the folder at `picture-frame.server` (saved to `listing.xml`, read as it streams by
`paperpaw_sdk::xml`; the X4's memory body is 44 KiB, a listing may be longer), then a `GET` of each photo
not cached, one at a time, each saved to `cache/p<hash>.jpg|png` (the host writes `.pp-staged` and renames, so
a cut leaves the old file). Basic authentication from the user and the password (a secret, `password`),
built into one header held while a sync runs and wiped. **Cache cap** (`cache-mb`, 64 at first): the plan
(`native_cache::plan`) takes the photos that fit the cap starting at `window`; where the server has more
than fits, each sync starts after the last photo of the one before, so over the days every photo comes up.
Cached files the plan no longer wants are removed before fetching, so the cap holds at every moment but
the one photo in flight (`max_file` is the room left). The list of the cache is `cache.txt` (written after
each photo).

**Power:** nothing here turns Wi-Fi on for a photo and nothing runs at a wake. A fetch starts only
(a) on request (*Fetch photos from the server* in the home screen, the action `picture-frame.sync`),
(b) when the charger is plugged in (setting `sync` *With the charger*, the first value, at most every six
hours) or (c) under *With the charger and when online*, when Wi-Fi is up anyway (`WifiChanged` connected
for other reasons, at most daily). A request made without a network waits for one (`network.md` 3.2: the
network manager decides; `offline` and `saving` end it at once) and ends with an explicit message.

## Data (`docs/data-files.md`)

`cache/` (the photos), `cache.txt` (`name<TAB>size<TAB>address`), `state.txt` (`cursor`, `window`, `synced`),
`listing.xml` (a listing being read, removed after it).

## Not done / device-only

- Not run against a real Nextcloud, ownCloud or NAS, nor on a device (X4, X4 Pro: not run; the S3 file
  was not built here, the Xtensa linker is not installed; the X4 file is 222,544 B and the packer accepted
  its frames): the tests use an in-process fake server (`native_sync_tests.rs`). On the emulator
  (`tools/pps/picture-frame/run-emulator.sh`, a fake WebDAV server `testdata/fake_webdav.py` with an
  account, three photos on the card and three on the server) the home, the slideshow with its keys, the
  sleep screen, a refused account (401) and a fetch with the account pass, the cache is listed and the
  slideshow shows the six photos: 16 checks pass. The emulator is built for the computer with the
  author's route (`paperpaw-dev build --device emulator`): the libraries of `tools/plugin-host-libs` have
  no heap word, so a plugin cannot allocate in them. Basic authentication only (no digest, no OAuth app passwords' flow: a
  Nextcloud app password works as the password); no `Depth: infinity`, no subfolders.
- Photos above 8 megapixels (a 12 MP phone photo) are refused by the host's decoder (`TooLarge`) and passed
  over for the next photo after at most six tries; the card folder is best filled with photos scaled down.
- The sleep screen is the one in the slot while it is chosen; a device whose sleep screen the reader did not
  choose here keeps its own and no photo is prepared (a prepared photo takes a decode and a cache file).
- An orphan `.pp-staged` file of a cut download is replaced by the next fetch of the same photo; none is
  swept (the data folder cannot be listed by the plugin).
- GIF is not offered (the host decodes only its first frame).
- A photo that cannot be shown leaves the white screen and the words *Loading the photo* until the next
  tick or choice; the failing photo is passed over (six tries).
