//! The kernel-owned Literata upgrade dialog (a host screen,
//! `host_screen::LITERATA`).
use paperpaw_sdk::Id;

pub(crate) const SCREEN: Id = Id::new_static("core.literata-notice");
pub(crate) const ANSWERED: Id = Id::new_static("core.literata-pack-answered");
