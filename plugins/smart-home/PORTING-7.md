# Smart home on interface 7

Status: **complete (Home Assistant REST first).** A new plugin made for interface 7 (draft 36, no interface
change); there is no interface-6 part. Version 1.0.0. A store plugin (`tools/plugin-files/plugins.txt`,
format-5 file by `tools/plugin-files7`), linked and shipped off in the emulator (`native-linked-store`), not
linked into the firmware. The name is neutral: Home Assistant is named where the plugin is described.

## What it does

- **Connection** (`smart-home.connect`, also the editor of the setting `smart-home.server`): the server's address
  and the long-lived access token, which is kept among the plugin's secrets (`core.secrets`, name `token`), never
  in a settings file or a log; the screen shows only whether one is kept. *Test connection* fetches and says
  how many entities the server lists, or why not.
- **Addresses:** only the home network is used by default (`link::is_home`: private, loopback and link-local
  addresses, `localhost`, one-word names, `.local`, `.lan`, `.home`, `.home.arpa`, `.fritz.box`, `.internal`).
  Plain `http://` is used there only. A server elsewhere needs the advanced setting *Servers outside the home
  network* and `https://` (the host's roots decide: a self-signed certificate fails as *tls*, said in words).
- **Entities** (`entities.txt`, at most 24): chosen on the picker. A search over the server's list: the whole
  `/api/states` answer is **saved to `states.json`** (`save_to`, no memory limit) and read back in pieces of
  1,536 bytes by `states::States` (the SDK's fallible JSON parser, one record at a time), so the heap holds the
  record being read and the [`pick::LIMIT`] (40) matches kept, whatever the server's size. A refresh keeps only
  the chosen ones. Locks, alarm panels, climate and weather are left out on purpose (a button on a shelf).
  Each entity may get a *room*, which groups it on the dashboard and in the widgets.
- **Dashboard** (*Smart home* in the tools, action `smart-home.open`): rows under the room headings; a value
  reads (numbers rounded to two decimals, doors as *Open/Closed*, presence as *Detected/Clear* by device class);
  selecting a light, switch, fan, cover, automation or scene calls `/api/services/<domain>/toggle|turn_on|press`
  with `{"entity_id": ...}`. The row shows *…* while the call is out and then **the state the server answered
  with** (the changed states are in the answer; asked for with `GET /api/states/<id>` where it is not), plus a
  notice. A changed state changes that row's text in place and only that row is drawn again (partial refresh);
  the rows are made again only where entities or rooms change.
- **Widgets** (all four draw by pixel room, `core.widgets.world`): *value* (name and one reading large), *group*
  (the entities of a room or all, a row each, as many as fit, `+ n more`), *switch* (state word, a tap switches)
  and *room* (a card: the room's readings in one line, its switches below). The widgets are told only when a
  digest of what they show changed.

## Power and the radio

Nothing here turns Wi-Fi on for the plugin's own fetching and nothing holds it on. The states are read (a) when
the dashboard opens and (b) on request (the Refresh row, the action, a tap on a value or group widget), both of
which wait for the network as any request does (`network.md` 3.2); (c) at a wake **only if Wi-Fi is up already**,
something shows the states and they are older than two minutes; (d) on a schedule **while charging**
(`ChargeChanged`, setting `refresh`: never, 15, 30 at first, 60 or 120 minutes; one `core.work` with
`when_cheap`, `Moment::Online`) and only while a widget is placed. The fetch of (c) and (d) checks the Wi-Fi
state first and sends nothing otherwise. A request has 30 s; a call 15 s.

## Errors in words (en and de)

`link::Problem`: no address, no token, an address that is none / outside the home network / plain http outside,
no network, offline mode, network off for the plugin, the server did not answer, 404, TLS, the token not
accepted (401/403), another status (with the number), too large, the card, an answer that is no list of states.
On the dashboard they are the note below the rows; where the reader asked they are also a notice (not on the
connection screen, whose note says it).

## Data (`docs/data-files.md`)

`entities.txt` (v1: `v1<TAB>synced`, then `id name room state unit class` tab separated, texts cut and cleaned so no
escaping is needed); `states.json` (the answer being read, removed after it; kept while the picker is open).

## Tests

- Unit tests: `model` (kinds, services, ids, number rounding, state wording, the file round trip and its damaged
  forms, the cap), `states` (a 1,200-entity list in pieces of 1, 7, 64, 1,536 and 3,840 bytes, a single object, a
  service answer, a failing sink, a file read in pieces), `link` (home addresses, the base address, the calls, the
  failure words), `pick` (the first 40 in order of id and the rest counted, search by name and id, ticks), `dash`
  (rows by room, a changed state changes its row only), the secret type, the catalogs.
- `tests/fake_ha.rs` against `testdata/fake_ha.py`, a real HTTP server on the loopback with Home Assistant's
  shapes and a bearer token (skipped without python3): a 600 KB list of 1,511 states read in pieces with the four
  chosen entities kept and nothing else, a wrong token (401 in the server's words), toggle / turn_on calls and the
  answers that confirm the state, a single state, a malformed call (400).
- `tools/pps/smart-home/run-emulator.sh` on the emulator against the same server (611 entities): the plugin is
  switched on, the empty dashboard, a refused and then an accepted token, the picker (search, rooms), the dashboard's
  readings and a light switched off and on (two POSTs seen in the server's log), the four widgets on Home, and
  `entities.txt` without the token. 7 tests pass.
- The scripted host has no data folder, so the plugin's flow with the files is covered by the emulator run.

## Not done / device-only

- Not run on an X4 or X4 Pro. The X4 file builds and packs (`tools/plugin-files7/build.sh x4 --only smart-home`:
  229,972 bytes, 40,272 of data, 184,924 of code); the S3 file was not built here (no Xtensa linker).
- Home Assistant is the only server (REST). No WebSocket, no area registry (so no automatic rooms: the room is
  typed once for each entity), no history, no climate or lock control, no HTTPS to a self-signed certificate.
- Real Home Assistant was not available: the fake server follows the documented REST API.
