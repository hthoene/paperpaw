//! PaperPaw's logo, the paw on a sheet with a dog-ear, where screens show
//! it (the sleep screen, the logo widget): drawn as shapes in any size,
//! sharp on every panel, nothing kept in memory.
//!
//! A picture takes its place where [`LOGO`] names one (a JPEG, PNG or GIF
//! on the card), e.g. one a data plugin brings with
//! `[core] logo = plugins/mine/logo.png` in its `settings.ini`.

use crate::setting::{Setting, Text};

/// A picture shown in place of the drawn logo; none (empty) for that.
pub const LOGO: Setting<Text> = Setting::new("core.logo");
