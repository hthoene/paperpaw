# Interface 7: core events (EVT)

Status: contract of family 1b (#228). Everything here is appended: event records 64-73 in the
`event` growth block (`screens-input.md` section 8). The records change the SDK `Record` shape, so
this is the next draft (13). Items marked [D] are decided.

It builds on the event rules already in place: registration with `registration.event`, one record
per `dispatch_event`, borrowed input valid for the call only, held back during a callback, and the
failure rules of `proposals/interface-7-events-storage-services.md` section 1.5.

## 1. Scope

Interface 6 has about 60 built-in events. Family 1b carries the host-produced broadcasts that ported
plugins subscribe to most and that no later family owns:

| Interface 6 | Uses in `plugins/` | Interface 7 |
| --- | ---: | --- |
| `core.book-opened`, `-closed`, `-finished` | 40, 24, 14 | 65-67 |
| `core.setting-changed` | 31 | 64 |
| `core.book-moved`, `-removed` | 13, 9 | 68, 69 |
| `core.plugins-changed` | 12 | 70 |
| `core.wifi-changed` | 8 | 71 |
| `core.status-changed` | 7 | 73 |
| `core.power-changed` | 1 | 72 |

- [D] **Already in interface 7:** started, resume, suspend, stopping, battery measured, page turned
  (32-37), screen shown and action run (38, 39, delivered to their declaration's token).
- [D] **Plugin-to-plugin events** (`NOTES_EXPORTED`, `READ_TODAY`, `WORD_LOOKED_UP`, `GOTO`, the
  lookup and skim pairs) are stages (`stages-services.md`, N4); request and answer pairs
  (`PLUGINS_REQUESTED`/`ANSWERED`, `CATALOG_WANTED`) are services.
- [D] **Other families add their own events** in this block when they land: library and catalogue
  (`book_added`, `book_changed`, `annotations_changed`, `library_changed`, `catalog_*`: LIB),
  Wi-Fi scan and failure, Bluetooth, firmware and USB drive (DEV), network demand (NET), work (ASY),
  documents (DOC), menus, focus and display (UIX).

## 2. Records

All fields are required unless marked optional; `now_ms` is first, as in every event.

| Tag | Record | Fields |
| --- | --- | --- |
| 64 | `event.setting_changed` | `now_ms: u64`, `key: str 128`, `old: bytes 512` |
| 65 | `event.book_opened` | as `event.page_turned`: `now_ms`, `section`, `offset`, `progress_ppm`, `path: str 512`, `title: str 256`, `chapter: str 256`; optional `page`, `pages`, `insert` |
| 66 | `event.book_closed` | the same fields |
| 67 | `event.book_finished` | the same fields |
| 68 | `event.book_moved` | `now_ms: u64`, `from: str 512`, `path: str 512` |
| 69 | `event.book_removed` | `now_ms: u64`, `path: str 512` |
| 70 | `event.plugins_changed` | `now_ms: u64` |
| 71 | `event.wifi_changed` | `now_ms: u64`, `state: u32` |
| 72 | `event.power_changed` | `now_ms: u64`, `flags: u32` |
| 73 | `event.status_changed` | `now_ms: u64` |

- [D] **`event.setting_changed`:** `key` is the setting's full id (`<plugin>.<name>`, or `core.*`).
  `old` is the previous value in the settings value encoding (`settings_value`), empty when there
  was none. A setting value is never empty, so empty means "no previous value". The new value is
  read as usual (slot 16). The event goes to every subscriber, for every setting, except:
  - a secret-scoped setting (SEC family) is never announced to another plugin; its owner gets the
    event with `old` empty;
  - a change the plugin made itself through slot 15 is announced to it too, after its callback
    returns (no re-entry).
- [D] **Book events (65-67)** carry the reading position at that moment, with the field rules of
  `event.page_turned` (`screens-input.md` section 9, `records.md`): `progress_ppm` at most
  1,000,000, page counters absent until counted, never a present 0 or a page past `pages`. The SDK
  decodes all four into one `Reading` view. `book_finished` follows the last page turn of the book
  and comes before `book_closed`.
- [D] **Paths** in book events are paths on storage root 1 (`/`-separated, as `event.page_turned`),
  cut at a UTF-8 boundary to the cap. `book_moved` names the old and the new path; a book moved out
  of root 1 is `book_removed`.
- [D] **`event.wifi_changed.state`:** 0 off, 1 on and not connected, 2 connecting, 3 connected.
  Other values are reserved: a consumer treats them as "not connected".
- [D] **`event.power_changed.flags`:** bit 0 charging, bit 1 external power present, bit 2 battery
  low (the host's threshold). Other bits are 0 now; consumers ignore bits they do not know.
- [D] **Hints:** 70, 71, 72 and 73 say "this state changed, read it again". They carry the new
  state where it is one number (71, 72); everything else is read through the owning service
  (plugin inventory: REG, status items: the status stage). 64-69 are edges: each one happened.

## 3. Delivery

- [D] A plugin subscribes with `registration.event { event_tag, token }`, as for 32-37.
- [D] **Admission.** A host admits a tag in this family once it has the producer. Until then a
  registration naming it is `ERR_UNSUPPORTED` for the batch, as for any tag without an encoder.
  A host that produces a tag admits it on every device, even where it never fires (Wi-Fi on a
  device without Wi-Fi): a plugin's registrations never depend on the device.
- [D] **Order.** Events reach a plugin in the order the host produced them, one `dispatch_event`
  each. Events raised during a plugin's callback are delivered after it returns.
- [D] **Edges (64-69) are never coalesced.** One that cannot be delivered (the plugin is busy,
  stopping or its callback fails) takes the `event_dropped` path of section 1.5 of the events
  proposal and is logged; it is not retried.
- [D] **Hints (70-73) may be coalesced.** At most one of each tag is pending per plugin; a newer
  one replaces it (with the newer state). A dropped hint is not reported.
- [D] **Lazy start.** A plugin that is not running gets no events; registering for one does not
  start it. A plugin that must follow the library while stopped reads the state at its next start.
- [D] **Cost on the X4.** The host encodes into a stack buffer (the largest record, 65-67, is under
  1.1 KB, as `event.page_turned`); nothing is allocated per event. Per plugin and tag one
  registration entry.

## 4. Hand-offs

**Core:** producers for 64-73 from the kernel's interface-6 emitters (`Kernel::emit` sites of the
same events), admission of each tag in `native_host::events::is_event_tag` once its producer is
wired, encoders with the field rules above, coalescing of 70-73, and tests per tag (admission,
order, coalescing, a dropped edge reported).

**Services:** typed SDK decoding: `Event::SettingChanged { key, old }`, `Event::Book(kind,
Reading)`, `Event::BookMoved`, `Event::BookRemoved`, `Event::PluginsChanged`,
`Event::WifiChanged(WifiState)`, `Event::PowerChanged(PowerFlags)`, `Event::StatusChanged`.

**Kernel:** this contract and the records.
