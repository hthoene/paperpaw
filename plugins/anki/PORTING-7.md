# Anki (AnkiConnect) on interface 7

Status: **complete.** A new plugin made for interface 7 (draft 36, no interface change); there is
no interface-6 part. Version 1.0.0. A store plugin (`tools/plugin-files/plugins.txt`, format-5 file
by `tools/plugin-files7`), not linked into the firmware.

## Why a plugin of its own

Vocabulary keeps decks on the reader and schedules them itself (FSRS); with Anki the computer's
collection is the schedule and the reader only studies what Anki says is due and reports answers.
Putting both into `vocabulary` would have mixed two authorities, grown a 21,000-line plugin for
everyone and tied the sync's updates to it. This one is small, depends on `paperpaw-sdk` and the
ui-kit only, and a reader without Anki does not carry it.

## Protocol

AnkiConnect's classic API (version 6, `POST /` with `{"action","version","key","params"}`): the
current add-on (source: git.sr.ht/~foosoft/anki-connect, README and API_LEGACY.md) keeps it for
existing clients and every older add-on answers it, where the new JSON-RPC API (`/api/v1`, Bearer
key) needs Anki 26.08. Formats and protocol only; nothing of the add-on's code, and no AnkiWeb.
Actions used: `deckNames`, `findCards` (`is:due`, then `is:new`, suspended and buried left out),
`cardsInfo` (`question` and `answer`, rendered by Anki, so cloze and reversed cards are right) and
`answerCards` (ease 1 to 4). `cardsInfo` is asked without its `fields` filter, which older
add-ons do not know.

The key is sent in the body (`key`), kept among the secrets (`api-key`) and wiped from every buffer;
a request is never logged by the host. A reply is read as it streams (`paperpaw_sdk::json::stream`),
nothing larger than one card is in memory. The memory body of a reply is at most the host's buffer:
where `cardsInfo` is cut, the batch (3 cards) is halved down to one card, which is then left out.

## Data (`docs/data-files.md`)

- `cards.txt`: the cards of the last sync, `id<TAB>front<TAB>back`, texts escaped, at most 50.
- `answers.txt`: `id<TAB>ease` for each answer not yet sent (the offline queue, at most 32 KiB).
- `cards.new`, `answers.new`: staging files of a sync, replaced or removed at its end.

An answer is written at once; the cards done are those with an answer. A sync sends the queue first (40
answers a request), drops the cards (Anki's schedule moved), then fetches. What fails stays: the
answers wait, the old cards stay until a successful send made them stale.

## Not done / device-only

- Not run against the real add-on or on a device: the tests use an in-process stand-in
  (`native_sync_tests.rs`) and a fake server for the emulator (`testdata/fake_anki_connect.py`).
- A card answered *Again* returns with the next sync once Anki says it is due; there is no
  in-session repeat. The `/api/v1` API is not used.
- AnkiConnect's `webCorsOriginList` concerns browsers; a request without an `Origin` header is not
  checked against it.
