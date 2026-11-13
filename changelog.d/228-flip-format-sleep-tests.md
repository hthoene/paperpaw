### Changed

- Tests: with the linked reading plugins (the flip, #243) the sleep parity cases of the seven formats run, and the standby cycles require every plugin that may sleep, formats included; CI and `check-local.sh` run them with the reader and the formats linked (todo 566). The tests of the native reader (open last book, pictures in their own frame) and the network settings of native plugins run with the flip.
- Tests: with the native reader linked, the interface-6 page-layer, text-filter and decorator tests are skipped until interface 6 goes (#244), and the table of contents' partial redraw and the Wi-Fi rejoin of `--wifi online` are skipped with their open bugs named; the partial-redraw harness moves the native host's work clock, so the table of contents finds its chapters.
