### Added
- `core.net`: the shell can report a failed certificate or TLS handshake as `HttpReason::Tls`; the plugin gets `event.http_done` with reason 3 (`tls`, `network.md` 4.1).
