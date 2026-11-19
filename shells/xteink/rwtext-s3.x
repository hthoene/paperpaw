/* esp-hal 1.2.2's `rwtext.x` for the ESP32-S3 (its `#IF riscv` part left
   out, `use_rwtext_ld_hook` off), with one change (#387, as `rwtext-x4.x` for
   the X4 since #304): the Wi-Fi send and receive code (`.wifi0iram`,
   `.wifirxiram` and `.wifiextrairam`) is not kept in RAM here but runs from
   flash (`text-s3.x` puts it there): that much RAM more for the heap
   (the `.rwdata_dummy` of esp-hal's `esp32s3.x` counts the size of what is
   left). What runs in an interrupt or while the flash is written (`.iram1`,
   the sleep and PHY sections, esp-hal's own `.rwtext`) stays in RAM. Found
   before esp-hal's own: `build.rs` puts it first, as `rwtext.x`. Compare with
   esp-hal's file when esp-hal changes. */
.rwtext : ALIGN(4)
{
  . = ALIGN (4);

  _rwtext_start = .;

  /* Keep each object's literal pool adjacent to its code (see comment in `text.x`). */
  *(.rwtext.literal .rwtext .rwtext.literal.* .rwtext.*)
  /* unconditionally add patched SPI-flash ROM functions (from esp-rom-sys) - the linker is still happy if there are none */
  *:esp_rom_spiflash.*(.literal .text .literal.* .text.*)

  . = ALIGN(4);
} > RWTEXT

.rwtext.wifi :
{
  . = ALIGN(4);
  *( .wifislprxiram  .wifislprxiram.*)
  *( .wifislpiram  .wifislpiram.*)
  *( .phyiram  .phyiram.*)
  *( .iram1  .iram1.*)
  *( .coexiram.* )
  *( .high_perf_code_iram* )
  *( .coexsleepiram* )
  *( .wifiorslpiram* )
  *( .isr_iram* )
  *( .conn_iram* )
  *( .sleep_iram* )
  . = ALIGN(4);

  _rwtext_end = .;
  _rwtext_len = . - ORIGIN(RWTEXT);
} > RWTEXT
