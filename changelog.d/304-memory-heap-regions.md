### Changed

- **More free heap and fewer big one-piece blocks on the X4** (#304). The Wi-Fi send and receive code (16.7 KB) runs from flash instead of RAM, and the heap takes the RAM (16 KB). The small heap region is filled first at start-up, so the large one stays whole for big temporaries. An HTTP answer of known length is taken at that length, not at 32 KiB; a small PNG inflates into a window of its own size; a progressive JPEG probes its headroom in 8 KiB pieces, not one 48 KiB block; and a TLS session is opened only where a 20 KiB block is free.
