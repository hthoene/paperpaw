/* The Wi-Fi send and receive code the ESP32-C3 build (#304) and the ESP32-S3
   build (#387) run from flash, after `.text`: what esp-hal's `rwtext.x` kept in
   RAM (`rwtext-x4.x`, `rwtext-s3.x` leave these sections out). Espressif's own builds place them in flash
   where their `IRAM_OPT` options are off. Read after `linkall.x`
   (`build.rs`), whose `ROTEXT` it is. */
SECTIONS {
  .wifi_flash_text : ALIGN(4)
  {
    *( .wifi0iram  .wifi0iram.*)
    *( .wifirxiram  .wifirxiram.*)
    *( .wifiextrairam.* )
  } > ROTEXT
}
INSERT AFTER .text;
