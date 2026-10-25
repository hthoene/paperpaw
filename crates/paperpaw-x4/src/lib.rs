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
//! ([`provision`]), and the mark of a sleep that ends in a start
//! ([`slept`]).
//!
//! PaperPaw draws the screen upright, 480 × 800, in bands of rows; the
//! display writes them turned into the controller's memory, which keeps
//! the frame, so the firmware holds none (see [`display`]).

#![no_std]

extern crate alloc;

mod bands;
pub mod battery;
pub mod bm8563;
pub mod bounded;
pub mod bq27220;
pub mod buttons;
pub mod card;
pub mod changes;
pub mod cw2017;
pub mod dhcp;
pub mod display;
pub mod ds3231;
pub mod generation;
pub mod gt911;
pub mod nvs;
pub mod ota;
pub mod otp;
pub mod paced;
pub mod panel;
pub mod partitions;
pub mod probe;
pub mod provision;
pub mod remote;
pub mod sent;
pub mod slept;
pub mod sntp;
pub mod stall;
pub mod uc8179;
pub mod uc8253;
pub mod updates;
pub mod waveform;
