# Transfer over USB and plugins from a computer

A computer can install a plugin on a PaperPaw device, or add books to it,
over the USB cable or over the Wi-Fi transfer page. Both ways use the same
requests and the same install routine. Nothing is written before the
reader agrees on the device.

This page is for those who write such a computer program (the plugin
workshop, a page in a browser with Web Serial, a script). `tools/usb-transfer.py`
is a small client to read along with.

## The cable

The X4 and the X4 Pro speak over the USB serial link that the log and the
web flasher use (115200 baud, no reset on opening: keep DTR and RTS low).
The device is not a USB drive. The cable transfer is in every firmware,
not only in test builds.

Requests and answers are lines of text. A line that does not begin with
`PPT/1 ` is not the transfer's: the device's log comes on the same link,
and a client passes over it.

The computer sends a request:

```
PPT/1 <n> <METHOD> <target> <body bytes>
PPT/1 <n> data <hex>
PPT/1 <n> data <hex>
```

`<n>` is the computer's own number for the request; the answer carries it.
The body follows as `data` lines of at most 512 bytes each, in
hexadecimal, until as many bytes came as the first line said. A request
without a body has `0` and no `data` lines.

The device answers:

```
PPT/1 <n> <status> <content type>
PPT/1 <n> data <hex>
PPT/1 <n> end
```

The body comes in `data` lines of at most 512 bytes. Answers are JSON
unless the status says otherwise. One request at a time may have a body
coming; send the next request once the answer to the last one came. A
request sent while a body still comes cuts that body short: its request
is answered *400*.

When the first request comes, the device opens its USB transfer screen.
A request without a body waits for the screen (up to five seconds); one
with a body that comes before the screen is up is answered *503*, so a
client begins with `GET /api/hello` and sends bodies once it was answered.
It stays open while requests come, and closes after five minutes without
one, or when the reader presses *Back*. After *Back* a computer that goes
on asking gets *403* "ended on the device"; only a request after a pause
of ten seconds opens the screen again.

## Requests

`GET /api/hello` needs no permission and says what the device is:

```json
{"protocol": 1, "device": "xteink-x4-pro", "name": "Xteink X4 Pro",
 "version": "0.5.0-beta.1", "fingerprint": "0002c27c20e5fe94"}
```

`fingerprint` is the build that plugins with code must be made for (its
first four digits are the plugin interface), or `null` where the device
runs no plugins with code.

`GET /api/plugins` lists the plugins installed:

```json
{"plugins": [{"id": "airy", "version": "1.0.0", "on": true, "origin": "local"}]}
```

`origin` is `store` for a plugin the store installed (and updates),
`local` for one that came any other way, `built-in` for PaperPaw's own. (The
interface-7 Wi-Fi transfer reads the registry, which has no origin: it says
`built-in` or `installed`.)

### Asking first

Before anything is written, the computer says what it means to send:

```
POST /api/offer
```

with a body of lines:

```
plugin my-plugin
plugin.ini 112 <sha256>
i18n/de.ini 40 <sha256>
code.ppn 20480 <sha256>
book Some book.epub 512000 <sha256>
```

`plugin <id>` names a plugin, and each line after it a file of it with
its size in bytes and its SHA-256 in hexadecimal; a plugin's paths have no
spaces. `book <name> <size> <sha256>` names a book (a file name without
folders, spaces allowed). One plugin per offer, with at most 64 files,
and at most 64 books (send more in the offers after); each line of at
most 320 bytes. The device reads the offer a line at a time.

The device shows the names and sizes and asks the reader: *Allow this
computer to install a plugin / add books?* For a plugin with code it adds
that the code comes from this computer, not from the store, and runs with
full access to the device. The answer is *202* `{"state": "asking"}`;
*400* for an offer that names what may not be written (a path out of the
plugin's folder, a hidden file, a folder for a book); *409* while another
offer is asked about, or while the Wi-Fi transfer, open, is being sent
the same plugin; *507* where there is not room for all of it. Files of
the plugin sent before the offer are let go.

The computer then asks `GET /api/offer` until the state is `allowed` or
`refused`. The permission holds for this offer, while the screen is open;
the question closed without an answer counts as `refused` (a screen shown
over it does not).

### Sending

Once allowed, only what was offered is taken, at the size and SHA-256
offered, each once:

- `PUT /api/plugin/<id>/<path>?sha256=<hex>`: a file of the plugin, its
  SHA-256 in hexadecimal (it may be left out: the offer's counts). *422*
  where the file is not what was offered.
- `POST /api/plugin/<id>`: installs the plugin from the files sent, in one
  step: `{"state": "installed", "restart": false}`. The version before
  stays until the new one is whole; the plugin's own data stays; it is
  installed only if the files sent are those offered, each with the
  SHA-256 offered, and no others (*422* `"not as offered"`); it is
  loaded at once. A plugin with code is checked whole and must be built
  for the device's `fingerprint`; it runs after a restart (`"restart":
  true`). *422* `{"state": "refused", "why": …}` where it was not
  installed; nothing installed changed then.
- `PUT /api/book/<name>?sha256=<hex>`: a book (`sha256` as for a plugin's
  file), into the library folder,
  under a free name: never over a book there.
  `{"path": "books/Some book (2).epub"}`.

Requests that would write anything else are refused (*403*): no other
files, no settings, no reading of files.

## Over Wi-Fi

The Wi-Fi transfer page (*Tools*, *Wi-Fi transfer*) takes the same plugin
requests, each with the page's key (`k=`), shown on the device:

- `GET /api/plugins`
- `PUT /api/plugin/<id>/<path>?sha256=<hex>`
- `POST /api/plugin/<id>`: installs; for a plugin with code the device
  asks the reader first, and the answer is *202* `{"state": "asking"}`;
  the answer holds for the code asked about only: code sent again while
  the question shows is not installed
- `GET /api/plugin/<id>`: how the last install stands (`none`, `asking`,
  `installed`, `refused`)
- `DELETE /api/plugin/<id>`: drops the files sent

The two ways keep their files apart. While the cable is sent a plugin,
the Wi-Fi transfer refuses files of that plugin (*409*).

## The emulator

`paperpaw-emu --cable` speaks the cable transfer on standard input and
output, as a device does over its cable; in its window the questions show
and are answered as on a device. With `--headless` it answers what needs
no permission. For example:

```
python tools/usb-transfer.py --emulator "paperpaw-emu --cable" install path/to/my-plugin
```
