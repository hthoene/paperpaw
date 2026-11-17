### Changed

- **A smaller firmware: identical functions folded and TLS trimmed** (#304). The X4's linker folds functions with identical code. mbedTLS drops TLS 1.3, SHA-1, the CBC suites, RSA key exchange and the P-521 curve: HTTPS keeps TLS 1.2 with ECDHE and AES-GCM or ChaCha20-Poly1305 (X25519, P-256, P-384), which every server the firmware knows negotiates; a server that offers only TLS 1.3 or only CBC suites no longer connects.
