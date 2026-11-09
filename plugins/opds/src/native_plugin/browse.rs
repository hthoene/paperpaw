//! Browsing a catalog (the screen of `native_browse`).
use alloc::vec::Vec;

use paperpaw_sdk::Handled;
use paperpaw_sdk::native::{
    Error, Host,
    callbacks::{DrawFrame, Input},
    image::ImageReady,
    net::{HttpDone, HttpProgress},
    ui::Instance,
};

use super::NativeOpds;

/// The browse screen.
#[derive(Debug, Default)]
pub(super) struct BrowseView;

impl BrowseView {
    pub(super) fn draw(&mut self, _host: &Host<'_>, _frame: DrawFrame<'_>) -> Result<(), Error> {
        Ok(())
    }
}

impl NativeOpds {
    pub(super) fn opened_browse(
        &mut self,
        _host: &Host<'_>,
        _instance: Instance,
    ) -> Result<(), Error> {
        Ok(())
    }

    pub(super) fn browse_input(
        &mut self,
        _host: &Host<'_>,
        _token: u32,
        _input: &Input<'_>,
    ) -> Result<Handled, Error> {
        Ok(Handled::No)
    }

    pub(super) fn http_done(&mut self, _host: &Host<'_>, _done: &HttpDone) -> Result<(), Error> {
        Ok(())
    }

    pub(super) fn http_progress(
        &mut self,
        _host: &Host<'_>,
        _progress: &HttpProgress,
    ) -> Result<(), Error> {
        Ok(())
    }

    pub(super) fn image_ready(
        &mut self,
        _host: &Host<'_>,
        _ready: &ImageReady,
    ) -> Result<(), Error> {
        Ok(())
    }

    pub(super) fn close_browse(&mut self, _host: &Host<'_>, _instance: Instance) {}

    pub(super) fn terms_typed(&mut self, _host: &Host<'_>, _text: &str) -> Result<(), Error> {
        Ok(())
    }

    pub(super) fn read_saved(&mut self, _host: &Host<'_>) -> Result<(), Error> {
        Ok(())
    }

    pub(super) fn stop_all(&mut self, _host: &Host<'_>) {}

    pub(super) fn sync_action(&mut self, _host: &Host<'_>) -> Result<(), Error> {
        Ok(())
    }

    pub(super) fn work_due(&mut self, _host: &Host<'_>, _key: &str) -> Result<(), Error> {
        Ok(())
    }

    pub(super) fn sync_running(&self) -> bool {
        false
    }

    pub(super) fn sync_row(&mut self, _host: &Host<'_>) -> Result<(), Error> {
        Ok(())
    }

    pub(super) fn sync_one(&mut self, _host: &Host<'_>, _index: usize) -> Result<(), Error> {
        Ok(())
    }

    pub(super) fn sync_report_text(
        &self,
        _host: &Host<'_>,
        _say: &mut paperpaw_ui_kit::Messages,
    ) -> Result<Option<alloc::string::String>, Error> {
        Ok(None)
    }
}

#[allow(dead_code, reason = "the browse screen holds rows")]
fn _unused(_: Vec<u8>) {}
