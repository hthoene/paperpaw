//! The kinds of [`Contribution`]: what the kernel applies of a plugin.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use core::fmt;

use crate::fonts::{FontFace, FontSpec};
use crate::formats::FormatSpec;
use crate::id::Id;
use crate::menus::{MenuEntry, MenuNeed, MenuSection};
use crate::plugin::{Priority, SlotSpec};
use crate::presets::Preset;
use crate::settings::SettingSpec;



