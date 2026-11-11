//! The Xteink X4's and X4 Pro's hardware, for PaperPaw's firmware: their
//! displays (the SSD1677 in [`display`], the UltraChip UC8179 and UC8279
//! in [`uc8179`], whichever [`probe`] finds, driven as a [`panel`]), the
//! X4's microSD card, buttons and battery, and the X4
//! Pro's touch screen ([`gt911`]), fuel gauge ([`cw2017`]) and clock
//! ([`bm8563`]). Written against
//! `embedded-hal`, so all of it is tested on a computer, against
//! simulated hardware; the firmware only connects the chip's pins to it
//! (which pin does what: the firmware's `board` module,
//! `shells/xteink/src/board.rs` for the X4, `board_pro.rs` for the X4
//! Pro). Also what any X4 firmware does alike
//! and can be tested here: remote control over USB ([`remote`]), the
//! frame kept for the panel ([`sent`]), the clock set over the network
//! ([`sntp`]), addresses in the device's own network ([`dhcp`]),
//! updates over the air ([`ota`], run as the firmware does in
//! [`updates`]), handles of the network stack made
//! anew ([`generation`]), waits that never hang ([`bounded`]) and Wi-Fi
//! given with the firmware
//! ([`provision`]), the mark of a sleep that ends in a start
//! ([`slept`]), and what the firmware's TLS sessions for plugins' sockets
//! decide: who holds the few slots ([`tls_gate`]), which certificates are
//! trusted and what a failed handshake comes to ([`tls_trust`]), and how
//! long they may take ([`tls_watch`]).
//!
//! PaperPaw draws the screen upright, 480 × 800, in bands of rows; the
//! display writes them turned into the controller's memory, which keeps
//! the frame, so the firmware holds none (see [`display`]).

#![no_std]

extern crate alloc;

mod bands;
pub mod battery;
pub mod blame;
pub mod bm8563;
pub mod body_room;
pub mod bounded;
pub mod bq27220;
pub mod buttons;
pub mod card;
pub mod changes;
pub mod controls;
pub mod cw2017;
pub mod device_depth;
pub mod device_facts;
pub mod device_host;
pub mod dhcp;
pub mod display;
pub mod ds3231;
pub mod dual_boot;
pub mod escape;
pub mod flash_room;
pub mod generation;
pub mod gt911;
pub mod install_note;
pub mod light_timer;
pub mod mdns;
pub mod net_lost;
pub mod nvs;
pub mod ota;
pub mod otp;
pub mod paced;
pub mod panel;
pub mod partitions;
pub mod premake;
pub mod probe;
pub mod provision;
pub mod remote;
pub mod rtc_guard;
pub mod s3_band;
pub mod sent;
pub mod sleeping;
pub mod slept;
pub mod sntp;
pub mod stack_band;
pub mod stack_note;
pub mod stall;
pub mod stay_awake;
pub mod tls_gate;
pub mod tls_trust;
pub mod tls_watch;
pub mod uc8179;
pub mod uc8253;
pub mod updates;
pub mod upload;
pub mod usb_link;
pub mod waveform;
