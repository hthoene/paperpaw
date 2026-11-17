//! The kernel-owned Literata upgrade dialog (the shared start notice's screen).
use super::start_notice::StartNotice;
use alloc::boxed::Box;
use paperpaw_sdk::Id;

pub(crate) const SCREEN: Id = Id::new_static("core.literata-notice");
pub(crate) const ANSWERED: Id = Id::new_static("core.literata-pack-answered");

pub(crate) static SCREENS: [crate::KernelScreen; 1] = [crate::KernelScreen::new(
    SCREEN,
    "core.literata-pack.title",
    |env, params| crate::layer::old(Box::new(StartNotice::literata(env, params))),
)];
