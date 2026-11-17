//! Asking the reader for text, a folder, one of a few options or yes or
//! no, with the answer as a typed event ([`Typed`], [`FolderChosen`],
//! [`Picked`], [`Confirmed`]) the plugin hears with
//! `Items::on`:
//!
//! ```
//! use paperpaw_sdk::ask;
//! use paperpaw_sdk::event::Event;
//! use paperpaw_sdk::replies::Typed;
//!
//! const NAMED: Event<Typed> = Event::new("demo.named");
//!
//! // In an action or a screen:
//! let command = ask::text("Name of the shelf", &NAMED)
//!     .initial("Novels")
//!     .with("shelf", "3")
//!     .command();
//! // r.on(NAMED, |env, typed, out| { typed.text, typed.context.get("shelf") … })
//! # let _ = command;
//! ```
//!
//! The keyboard (slot [`KEYBOARD`](crate::builtin::slots::KEYBOARD)),
//! the folder chooser ([`CHOOSE_FOLDER`](crate::builtin::slots::CHOOSE_FOLDER)),
//! the choice ([`CHOICE`](crate::builtin::slots::CHOICE)) and the yes or
//! no question ([`CONFIRM`](crate::builtin::slots::CONFIRM)) are screens
//! another plugin can replace: they answer every question asked this way.
