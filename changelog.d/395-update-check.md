### Fixed

- The update check on the X4 no longer fails at once with "the request failed": it asked for an answer of up to 40 KiB, more than the X4 allows a memory body (16 KiB), so the request was refused before it was sent; it now asks for 16 KiB (#395).
- A failed secure connection on the X4 is now reported as such (certificate or TLS failure, `tls`) instead of "could not read or write". The catalog (OPDS) screen then says "Certificate not known. Install 'All certificates' from the store." instead of "could not be reached" (#395).
- A network request the host refuses leaves one line in the device log (`net: request of … refused (code)`) (#395).
