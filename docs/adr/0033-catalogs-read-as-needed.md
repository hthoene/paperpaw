# 0033. Catalogs on the card read as needed

- Status: Accepted (owner decision 40); amends plugin interface 5
- Date: 2026-11-01

## Context

A language that comes as a data plugin (`ar`, all plugins' texts, some
210 KB) is held whole in memory as a catalog: its text, an index of
2 bytes a message and its unescaped strings. Languages not in use stay
parked with their whole text. The X4 has 256 KiB of heap and starts
with some 125 KiB (140 in German) taken. A catalog of that size does
not fit, and those of every installed language cost memory for nothing.

Plugins borrow messages from the `Translator` the kernel lends them
(`Env::i18n`, `tr(&self, key) -> &str`), for as long as they hold it:
while they draw or handle an event. Between those calls the kernel holds
it alone.

## Decision

A catalog can stay on the card and be read as needed.

- **A compiled catalog** (`*.ppcat`, `paperpaw-sign catalog` makes it
  from an `.ini` catalog; `Catalog::compile`): a header, the `[meta]`
  text, then the messages sorted by a 32-bit hash of their key (FNV-1a),
  in blocks of at most 32 messages and 2 KiB, unescaped. A table of each
  block's first hash and place follows the header.
- **Opening it** (`Catalog::open`, on a `CatalogFile` that reads at a
  place) reads the header, the meta and the block table: 8 bytes a block,
  about 1 KB for 3,400 messages. It is checked whole (places in order and
  inside the file, blocks no larger than allowed, hashes in order); one
  that is not right is refused (`CatalogError::Corrupt`) and the language
  falls back as if it were missing.
- **Looking up** a key finds its block by the hash in memory and reads
  that block (one read of at most 2 KiB), then keeps the message, or
  that there is none, in the catalog's list of what it read. Further
  lookups of the key find it there. The list only grows while the
  `Translator` is lent out, so what a plugin borrowed stays where it is
  (an append-only list of `OnceCell`s, no `unsafe`); the kernel empties
  it (`Translator::forget_read`, with the `Translator` its own) when the
  screen on top changes, and after a call once it holds more than
  12 KiB. Beyond 16 KiB (`MAX_READ`), messages are not read any more
  until then: the language falls back for them.
- **Failing to read** (a card pulled, a read error, a block that is not
  right) gives no message: the next language of the chain (English,
  built in) or the key stands in, and nothing panics.
- **Languages not in use** keep their file and meta only (`unload`
  drops the block table), not their text.
- **What stays**: catalogs built into the firmware and plugins (in
  flash, their indexes in the program, `FixedIndexes`), catalogs given
  as text (`Registrar::add_translations`), and `.ini` catalogs on the
  card (a user's own, at most 64 KiB) keep working as they did. A data
  plugin's `i18n/*.ppcat` are opened from the card
  (`Registrar::add_translation_file`).
- **Covered English**: a catalog on the card does not count when the
  kernel checks whether a language has all of English's messages (it
  would read every block), so English stays: it is built in and costs
  no heap.

## Consequences

- Plugin interface 5 changes before its release (the owner's second
  re-bless): `paperpaw-i18n` gains `Catalog::open`, `Catalog::compile`,
  `CatalogFile`, `CatalogError::Corrupt`, `Translator::forget_read` and
  `Translator::read_bytes`, and `Catalog`'s shape grows. The kernel
  gains `Registrar::add_translation_file`. Every plugin is built again.
- A language of any size costs about 1 KB of index and the messages of
  the screen shown (a few KiB), not its size.
- The first lookup of a message costs a read from the card (a few
  milliseconds on the X4); a screen of 30 new messages opens up to some
  100 ms slower the first time. Pages of a book show few messages and
  turn as fast as before.
- `Catalog::keys` lists no keys of a catalog on the card (it would read
  the whole file); tools that compare catalogs read the `.ini` files.

## The batch it landed in

**Build from `e5f4a41a` or later.** The bless (`26bdc02c`) changed the plugin
fingerprint; shared bodies changed after it (digits, the right-to-left
mirroring of the shared UI, Hangul composition), which a fingerprint of
layouts and constants does not see. The pen and TLS reservations
(`e5f4a41a`, #81) changed the fingerprint once more: `PointerEvent` kept its
size, so the layout digest now also holds where its fields lie. The
table-compat baseline is `e5f4a41a`: every store, test-card and Workshop
build of this release comes from it or later, and builds of the commits
before are thrown away.

The owner put every queued interface change into the same re-bless of
interface 5 (2026-11-01, branch `kernel/iface5-catalog-ondemand`): one
commit each, with host tests, CHANGELOG lines and, in the commit
message, what other plugins and shells still have to do. The work left
for others, in one list:

**Plugin engineer**

- Sliders: settings page and menu panel make them with
  `.mirrored(style.is_rtl())`, hit-test with `Slider::parts`, step with
  `Slider::key_step` (#33).
- Digits: `fa`/`ur` catalogs say `digits = arabext` (`ar` `arab` where
  wanted); numbers written with `format!` (status line, library counts,
  stats, clock) go through `Translator::number`/`localize_digits` (#33).
- Catalogs: the `ar` catalog as `i18n/ar.ppcat` (formats 4) (#33).
- format-epub reads CSS `letter-spacing`, `text-decoration-style` and
  `overline`; the reader and page layers use `LineBidi::DROP_CAP`
  instead of the baseline heuristic (#27).
- The reader's "install the Greek pack" hint takes
  `Fonts::missing_letters` around a page (#60).
- The store hears `PLUGINS_REQUESTED` (a confirm sheet, then its queue)
  and fetches what `requires` names; templates send
  `Command::request_plugins`; the settings page says "set by …" from
  `Settings::default_from`; the plugin manager says what is missing
  (#72).
- The store pins a recovery key at first install, checks rotations with
  `check_rotation` and hands them to `accept_rotation` before it writes
  the install marks, passes the library's signed revocations
  (`keys::Revocation`) to `keep_revoked`; the Workshop makes and backs up
  the recovery key (#20, with the web side; threat model below).
- Offers to go online on background paths (weather, news, update
  check, kosync) send `Command::GoOnlineFor`; power settings follow
  `Capabilities::power` (#35).
- Downloads of known or bounded size set `HttpRequest::max_file`
  (store, read-later-cloud, news, catalogs) (#31).
- The transfer screen shows `WifiState::reach`; a "Connect to
  computer" entry and screen for the USB drive (#32).
- The reader's zoom asks for `Document::prepare_image_larger` (#28).
- RAM (#9): the 18 widgets that capture shared state, and the plugins'
  handlers that capture nothing (wifi, kosync, stats, rules, joplin
  first; about 100 `r.on` in all), move to `FixedWidget` and `hears!`
  lists; state they capture moves to statics or a service first.

**Threat model** (#20). Installed plugins are trusted: every plugin has the
whole card (owner decision 44: freedom over security; there is no
sandbox), so nothing here guards against a plugin on the device. The
author, recovery and revocation checks guard against outside attackers: a
rogue or compromised library or catalogue, the network between it and the
device, and forged or replayed statements. The kernel therefore believes
no data from outside without a signature it checks itself: a rotation must
be signed by the plugin's pinned recovery key, and a revocation by the
revoked key itself or by that recovery key, for a key pinned on this
device. It checks this both when it keeps an entry and when it reads its
lists from the card. The store only carries bytes.

**Firmware engineer**

- The X4 shells call `KernelBuilder::font_dir` so packs loose on the
  card show up (#59); `WifiState::Online::name` from mDNS (#32); the S3
  shells' USB mass storage behind a `usb-drive` feature (#32, owner
  decision on flash); Android reports `Battery::saver` and
  `PowerControls::system_saver` (#35).

**Kernel engineer, next phase (no interface change)**

- Tables: format-epub fills `TableRow`, the layout sets a row's cells in
  their columns, keeps a row on one page, repeats header rows and adds
  the rules; the reader draws `Page::rules` (#27).


## Interface 6 follow-up (#208)

The kernel also checks the 12 KiB threshold between commands and before
command rounds, once the preceding callback has returned every borrowed
message. Later commands can translate notices and dialog labels even if
an earlier callback filled the cache. Smaller caches remain resident.

The borrowed `&str` API still prevents eviction during a callback: a
plugin may retain any earlier message until that callback returns.
Reading more than 16 KiB within one callback can still fall back. An
owned or scoped text API that permits bounded eviction is a candidate
for the next interface round; the budget is not increased here.
