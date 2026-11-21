# Progress sync (kosync) on interface 7

Version 1.0.11. Native implementation; remaining host and device acceptance is listed below.
The store plugin is not linked into the firmware.

## Implementation

- Native declarations provide the account screen, user/automatic-sync/server settings, and
  *Sync progress* in Reader More. Screen lifecycle events arrive at the screen's own token;
  the plugin does not subscribe globally to ScreenShown.
- Library and Document service adapters obtain the KOReader partial-MD5 fingerprint,
  extract and locate XPointers, and close temporary documents on success and failure.
  Missing or unresolvable XPointers fall back to bounded section-length percentage lookup,
  including formats without XPointers. Busy errors remain retryable.
- Metadata declares Device, Document, Library, Net, Secrets, UI and Work services.
  Optional network capabilities permit loading without a network; actual host availability
  still determines whether a request can be made.
- The account supports sign-in, account creation, sign-out and server changes. The password
  derives the authentication key; the key is kept through Secrets with no plaintext fallback.
  Existing key migration and device-ID storage remain in native_store.rs.
- The engine fetches on open/manual sync, holds a farther remote position until a choice,
  throttles page sends, and handles close, suspend, resume and deferred push work. Native
  limits are four unsent books, four dismissed positions and five books left.
- Replies are bounded to 4,096 bytes; incomplete bodies are rejected. Request, key and
  service scratch buffers containing secrets are wiped as implemented by their owners.

## Evidence and remaining acceptance

- native_backing.rs tests exercise the Library fingerprint, XPointer/locate, non-XPointer
  percentage fallback, unknown-pointer fallback, Busy propagation and document closure.
- native_server.rs executes the real native entry against deterministic service replies:
  fetch/close push, farther-position hold/stay, signed-out refusal, malformed/truncated
  replies, and no Library position write for unaccepted offers.
- native_host.rs uses the real kernel and keyboard for login/account creation, a fresh-kernel
  restart on the same card, local sign-out, and another restart remaining signed out.
- native_menu.rs checks the shown native More-menu: *Sync progress* appears only while the
  plugin is installed and enabled. Kernel::menus() alone does not include native declarations.
- native_handover.rs confirms the offer closes into the real reader at 80 %, Back without
  a page turn stores 80 % on the card, and reopening retains that position without another offer.
  The exact test fails at the missing save without Reader's Start::At unsaved fix and passes
  with it. Body-text labels are not used as readiness evidence.
- Kernel's uncatalogued book_key fix (#3124, owner decision 199) and run_action dialog-close
  ordering fix (#266) are locally merged dependencies. The fixture opens the Reader slot;
  no catalog workaround or core test hook is used.
- Beta 3 policy applies: kosync never stores an unconfirmed remote position; the reader saves
  its reached position after accepted navigation. Gate and unsigned store/install artifact
  evidence is recorded with the PR head; pack/readback does not establish device acceptance.

## Differences and device-only checks

The offer uses the host confirmation dialog (*Go there* / *Stay here*); closing means *not now*.
Messages are notices, and the account is a list with a footer. Outgoing progress uses the
reading event's millionths, as agreed for Beta 4. Exact format-specific forward progress
(core.document op 17) still has no SDK wrapper; the inverse percentage fallback uses section lengths.

Only an authorized device session can establish card-plugin admission, host service availability,
already-online network HTTP, TLS/time/trust, reconnect/background work, real Secrets refusal and
persistence, reader navigation, heap/stack and power behavior. Real KOReader interoperability is not
established by the in-process fake transport.

The *Go online* offer is restored (#459, the shared `paperpaw_ui_kit::online::Offer`; `uses.wifi`):
where Offline mode holds back *Sign in* or *Create account*, the host asks once; *Go online* sends it
again (the host lets a request of the same callback through, #461), *Not now* says
there is no network. Automatic fetches and pushes stay quiet. native_host.rs drives it on the real host.
