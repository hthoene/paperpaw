/* esp-hal 1.2.2's `text.x` for the ESP32-S3 (its `#IF riscv` part left out),
   with one change (#387): the Wi-Fi send and receive code is linked at the end
   of `.text` (flash) instead of in RAM, see `rwtext-s3.x`. `GNU ld` finds no
   `.text` for an `INSERT` of ours (as `wifi-flash-x4.x` does for the X4's LLD),
   so the section is made here. Found before esp-hal's own: `build.rs` puts it
   first, as `text.x`. Compare with esp-hal's file when esp-hal changes. */
SECTIONS {

  .text : ALIGN(4)
  {

    /* The Espressif blobs (esp-wifi-sys / esp-phy / BT) are GCC-compiled and
       emit per-function `.literal.<f>` paired with `.text.<f>` within each
       object file. Pull each archive's sections in as a single block using
       the combined pattern: both GNU ld and LLD preserve input-file order
       within a single `*(...)` group, which keeps every function's literal
       pool within L32R reach of its own code regardless of total image size.
       Without these per-archive selectors, the catch-all below would bunch
       all literals at the start and push wifi text past the 256 KB L32R
       window. */
    *libpp.a:(.literal .text .literal.* .text.*)
    *libnet80211.a:(.literal .text .literal.* .text.*)
    *libwpa_supplicant.a:(.literal .text .literal.* .text.*)
    *libmesh.a:(.literal .text .literal.* .text.*)
    *libsmartconfig.a:(.literal .text .literal.* .text.*)
    *libphy.a:(.literal .text .literal.* .text.*)
    *libcoexist.a:(.literal .text .literal.* .text.*)
    *libcore.a:(.literal .text .literal.* .text.*)
    *libregulatory.a:(.literal .text .literal.* .text.*)
    *libespnow.a:(.literal .text .literal.* .text.*)
    *libwapi.a:(.literal .text .literal.* .text.*)
    *libprintf.a:(.literal .text .literal.* .text.*)
    *libbtdm_app.a:(.literal .text .literal.* .text.*)
    *libbtbb.a:(.literal .text .literal.* .text.*)

    /* Bare `.literal` (no `.<f>` suffix) holds the `global_asm!` literal
       pools used by the boot code (`__pre_init` / `__post_init`). Unlike
       Rust functions, those `.text.*` sections do not carry their own
       paired `.literal.<f>`, so place the bare pool first and pull the two
       consumers in right behind it — that keeps their L32R loads in reach
       regardless of total image size. */
    *(.literal)
    *(.text.__post_init)
    *(.text.__pre_init)

    /* Catch-all (mostly Rust code). rustc's Xtensa backend emits each
       function's `.literal.<f>` immediately before its `.text.<f>` within
       the object, so — like the per-archive blocks above — a single
       combined `*(...)` group keeps every literal pool within L32R reach
       of its code. Splitting literals and text into separate `*(...)`
       groups instead bunches all literals at the start and pushes late
       code past the 256 KB L32R window in large (radio) images. */
    *(.literal.* .text .text.*)
    /* The Wi-Fi send and receive code runs from flash (#387; esp-hal's
       `rwtext.x` kept it in RAM, `rwtext-s3.x` leaves it out). After the
       Rust code; their literals are in the same sections (esp-hal's
       `rwtext.x` names no literal sections for them), so the reach of L32R
       does not matter. */
    *( .wifi0iram  .wifi0iram.*)
    *( .wifirxiram  .wifirxiram.*)
    *( .wifiextrairam.* )
  } > ROTEXT

}