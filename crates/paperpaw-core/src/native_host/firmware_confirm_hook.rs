//! The firmware confirmation shown without a firmware host, for tests that
//! have none (the app's flows): its screen and words are the host's own
//! ([`super::firmware_confirm`]); only what asks is made up.
use super::firmware_confirm::{SCREEN, open_params, texts};
use super::firmware_install::Ask;
use crate::{Effects, Kernel};
use paperpaw_platform::{
    Instant,
    firmware::{Signer, Text},
};

impl Kernel {
    /// Opens the firmware confirmation (`core.firmware-confirm`) as an
    /// install of the image `project` `version` asked by the plugin named
    /// `asker` would: unsigned, and with the four points of another
    /// firmware where `foreign`. No machine waits for the answer: the
    /// screen closes on it and nothing is installed. `true` where it shows.
    pub fn show_firmware_confirm(
        &mut self,
        now: Instant,
        asker: &str,
        project: &str,
        version: &str,
        foreign: bool,
    ) -> (bool, Effects) {
        let ask = Ask {
            owner: 0,
            request: 0,
            project: Text::new(project),
            version: Text::new(version),
            signer: Signer::Unsigned,
            foreign,
            older: false,
            other_key: false,
            restart: false,
        };
        self.now = now;
        let said = texts(&self.shared.translator, asker, &ask);
        let before = self.stack.len();
        self.open(SCREEN, None, &open_params(&said, 0));
        let shown = self.stack.len() > before;
        (shown, self.finish())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
mod tests {
    use paperpaw_platform::Instant;

    #[test]
    fn the_confirmation_shows_without_a_firmware_host() {
        let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let (shown, _) =
            kernel.show_firmware_confirm(Instant(10), "update", "other-os", "9.9.9", true);
        assert!(shown);
        assert_eq!(
            kernel.active_screen().map(paperpaw_sdk::Id::as_str),
            Some("core.firmware-confirm")
        );
    }
}
