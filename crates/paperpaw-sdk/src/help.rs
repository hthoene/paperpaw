//! First-time help: what a screen's controls do, shown over it the first
//! time it opens (by the `help` plugin), calmly, once.
//!
//! A plugin names, for a screen of its own (or another's), a few hints:
//! a text each (a translation key), for touch screens, for devices with
//! buttons alone, or for both, and where it points, if anywhere: a zone of
//! the screen (in thousandths of its width and height, e.g. the reader's
//! tap zones), drawn as a frame with the text in it; hints without a zone
//! are listed in a box in the middle.
//!
//! The hints go to the point `POINT` as text, a line each:
//! `<screen> <input> <zone> <text key>`, `input` one of `any`, `touch`,
//! `buttons`, `zone` `x,y,w,h` or `-` (so data plugins and native ones can
//! add them alike). Readers switch them off (`help.show`) or have them
//! shown again (*Show hints again*, in the system settings).
//!
//! They show as the screen opens. A screen that first prepares what it
//! shows (the reader, opening a book) has them wait instead (`wait`),
//! and says when it is ready (`ready`): screens below another get no
//! timers, so it would not get ready under them.

use alloc::format;
use alloc::string::String;

use crate::{Id, RegisterError};







