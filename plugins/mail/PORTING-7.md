# Mail on interface 7 (slice 1, partly ported)

Status: **not complete.** The interface-6 plugin (`plugin.rs`, the sessions, the screens, the
store and the widgets) is still in place beside the native one. It is removed in the PR that
finishes the port. Nothing of Mail runs on the native host yet except the declarations.

## Ported in slice 1

- `declarations.rs`, `build.rs`, `native/metadata*`, `src/native_plugin.rs`: the group `mail.group`
  (interface 6: `mail`; every ported plugin names its group so), the pages `mail.account.1` to
  `3`, the 13 settings of each account and the three for all (42), all with their en/de texts
  unchanged. The password of an account is declared as a **secret**. The settings, the groups,
  the actions *open* and *write*, the Tools entry (order 47), the icons and the four screens are
  compared field by field with what the interface-6 plugin registers (`tests/parity.rs`); the
  settings are also a golden (`testdata/declarations.bin.v1`).
- The actions, the entry, the icons and the screens are built by `declarations::ui` and
  `declarations::screens` and are **not in the image**: they would open nothing.
  `metadata*.json` lists no feature yet (`uses.network`, `uses.secrets` and `uses.storage` join with
  the slices that make the calls).

## Not ported yet (each is an ignored case in `tests/acceptance_pending.rs`)

| Piece | Waits for |
| --- | --- |
| The connection (IMAP, SMTP, STARTTLS, TLS, pinned keys), `session.rs`, `driver.rs`, `job.rs` | the sockets host and facade (`docs/abi/sockets.md` is a draft) |
| The password read, used and wiped | the secrets facade (#367 is open) |
| The connection of a screen ending with it | the work service (`docs/abi/work.md`) |
| Choosing a provider fills in the servers (`provider.rs`) | the settings write and the `SettingChanged` event on the native side |
| Folders, list, reader, compose (`browser.rs`, `reader.rs`, `writer.rs`, `compose.rs`) | `core.ui` compose; the card through `core.files` |
| The store of what was fetched (`store.rs`, `render.rs`) | `core.files` and the card root |
| The five home widgets and their settings | the widget host call (Core todo 76) |
| The question *Trust this server?* | `Question::choice` and the sockets certificate event |

Every interface-6 module that is not listed above as made fallible stays as it is.
