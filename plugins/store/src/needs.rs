//! What a build needs of the firmware, as the library says it
//! (`interface`, `needs_api` and `families` of a build, #83), against
//! what this firmware provides ([`Native`]): a plugin rebuilt after the
//! firmware's table grew (a new family of shared functions, a newer
//! revision of one) has the same fingerprint as one built before, but
//! the firmware refuses it. The store does not offer such a build; the
//! plugin it is, where installed, stays as it is.
//!
//! The rule is [`Native::problem`]'s, on what the library names instead of
//! a file's header (a test keeps them equal). A library that names none
//! of it (an older one, or a build of format 3 and before) is taken as
//! before: the build fits.

use paperpaw_sdk::native::{INTERFACE, Native};

/// The families of shared functions a build or a firmware has.
pub(crate) const FAMILIES: usize = 8;

/// What a build says it needs, each only where the library names it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Needs {
    /// The plugin interface it was built for.
    pub interface: Option<u16>,
    /// The newest minor of that interface it uses, where `abi` is
    /// `7.<minor>`.
    pub minor: Option<u16>,
    /// The bytes of the firmware's table it uses.
    pub api: Option<u32>,
    /// The revision of each family it uses, 0 for the others; whole, or
    /// not taken at all (an odd list is not trusted for a part).
    pub families: Option<[u16; FAMILIES]>,
}

/// Whether a build runs on a firmware.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fit {
    /// It does, or the library does not say it does not.
    Yes,
    /// It needs more of PaperPaw than this device has: a newer one.
    Newer,
    /// It was built for an older interface or before a change of a
    /// family it would see: it is for another PaperPaw, as one of
    /// another fingerprint is, and not offered.
    Older,
}

/// What the firmware here provides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Provides {
    /// The newest minor of the interface it speaks; `None` where it is not
    /// known yet (`Native::minor`, draft 29): the minor is not checked.
    pub minor: Option<u16>,
    /// The bytes of its table.
    pub api: u32,
    /// The revision of each family it has (0: none).
    pub families: [u16; FAMILIES],
    /// The oldest revision of each family it still runs.
    pub oldest: [u16; FAMILIES],
    /// Whether the firmware checks families at all: it does on the X4's
    /// chip (RISC-V) only, as [`Native::problem`] does, so a build for the
    /// X4 Pro is not held back by families it never reads.
    pub checks_families: bool,
}

impl Provides {
    pub(crate) fn of(native: &Native) -> Self {
        Self {
            minor: None,
            api: native.api_size,
            families: native.family_revs,
            oldest: native.family_min,
            // The shell names the target it was built for; the chip is not
            // in `Native`, and the RISC-V one is the X4's.
            checks_families: native.toolchain.contains("riscv32"),
        }
    }

    /// How a build that `needs` this fits.
    pub(crate) fn fit(&self, needs: &Needs) -> Fit {
        let interface = needs.interface.unwrap_or(INTERFACE);
        if interface < INTERFACE {
            return Fit::Older;
        }
        let mut older = false;
        let mut newer = interface > INTERFACE
            || needs.api.is_some_and(|api| api > self.api)
            || needs
                .minor
                .zip(self.minor)
                .is_some_and(|(need, have)| need > have);
        let used = needs.families.filter(|_| self.checks_families);
        if let Some(used) = used {
            for (i, &rev) in used.iter().enumerate().filter(|(_, rev)| **rev != 0) {
                newer |= rev > self.families[i];
                older |= rev < self.oldest[i];
            }
        }
        // Newer wins where both hold, as the firmware says it.
        if newer {
            Fit::Newer
        } else if older {
            Fit::Older
        } else {
            Fit::Yes
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::PluginProblem;
    use paperpaw_sdk::native::{Chip, Header};

    fn provides() -> Provides {
        Provides {
            minor: Some(2),
            api: 400,
            families: [3, 2, 0, 0, 0, 0, 0, 0],
            oldest: [2, 1, 0, 0, 0, 0, 0, 0],
            checks_families: true,
        }
    }

    fn needing(api: u32, families: [u16; 8]) -> Needs {
        Needs {
            interface: Some(INTERFACE),
            minor: None,
            api: Some(api),
            families: Some(families),
        }
    }

    #[test]
    fn a_build_that_says_nothing_fits_as_before() {
        assert_eq!(provides().fit(&Needs::default()), Fit::Yes);
        // An odd list of families is not read; the rest still counts.
        let odd = Needs {
            families: None,
            api: Some(401),
            ..Needs::default()
        };
        assert_eq!(provides().fit(&odd), Fit::Newer);
    }

    #[test]
    fn what_the_firmware_lacks_is_a_newer_paperpaw() {
        let p = provides();
        assert_eq!(p.fit(&needing(400, [3, 2, 0, 0, 0, 0, 0, 0])), Fit::Yes);
        assert_eq!(p.fit(&needing(0, [2, 0, 0, 0, 0, 0, 0, 0])), Fit::Yes);
        // A table bigger than the firmware's; a family revision past its;
        // a family it does not have.
        assert_eq!(p.fit(&needing(404, [0; 8])), Fit::Newer);
        assert_eq!(p.fit(&needing(400, [4, 0, 0, 0, 0, 0, 0, 0])), Fit::Newer);
        assert_eq!(p.fit(&needing(400, [0, 0, 1, 0, 0, 0, 0, 0])), Fit::Newer);
        // A later interface.
        let later = Needs {
            interface: Some(INTERFACE + 1),
            ..Needs::default()
        };
        assert_eq!(p.fit(&later), Fit::Newer);
    }

    #[test]
    fn what_was_built_before_the_oldest_still_run_is_not_offered() {
        let p = provides();
        assert_eq!(p.fit(&needing(0, [1, 0, 0, 0, 0, 0, 0, 0])), Fit::Older);
        let before = Needs {
            interface: Some(INTERFACE - 1),
            ..Needs::default()
        };
        assert_eq!(p.fit(&before), Fit::Older);
        // Newer wins where both hold.
        assert_eq!(p.fit(&needing(500, [1, 0, 0, 0, 0, 0, 0, 0])), Fit::Newer);
    }

    /// The firmware's own check says the same of a file that needs this,
    /// on the X4's chip (which checks families) and the X4 Pro's (which
    /// does not).
    #[test]
    fn it_says_what_the_firmware_says_of_the_file() {
        for (chip, toolchain) in [
            (Chip::Esp32C3, "rustc 1.99.0, riscv32imc-unknown-none-elf"),
            (Chip::Esp32S3, "esp 1.97.0.0, xtensa-esp32s3-none-elf"),
        ] {
            let mut native = Native::new("code.ppn");
            native.toolchain = toolchain.into();
            native.api_size = 400;
            native.family_revs = [3, 2, 0, 0, 0, 0, 0, 0];
            native.family_min = [2, 1, 0, 0, 0, 0, 0, 0];
            let provides = Provides::of(&native);
            assert_eq!(provides.checks_families, chip == Chip::Esp32C3);
            let base = Header {
                chip,
                fingerprint: native.fingerprint.0,
                base: 0,
                image_len: 0,
                code_at: 0,
                entry: 0,
                api: 0,
                relocs: 0,
                align: 4,
                compiler: 0,
                hot_at: 0,
                hot_len: 0,
                patches: 0,
                needs_api: 0,
                families: [0; 8],
                interface: INTERFACE,
                sha256: [0; 32],
            };
            for (api, families) in [
                (400, [3, 2, 0, 0, 0, 0, 0, 0]),
                (404, [0; 8]),
                (400, [4, 0, 0, 0, 0, 0, 0, 0]),
                (400, [0, 0, 1, 0, 0, 0, 0, 0]),
                (0, [1, 0, 0, 0, 0, 0, 0, 0]),
                (0, [2, 1, 0, 0, 0, 0, 0, 0]),
            ] {
                let file = Header {
                    needs_api: api,
                    families,
                    ..base
                };
                let problem = native.problem(&file);
                let fit = provides.fit(&needing(api, families));
                let said = match fit {
                    Fit::Yes => None,
                    Fit::Newer => Some(PluginProblem::NeedsNewerPaperPaw),
                    Fit::Older => Some(PluginProblem::OlderInterface {
                        built_for: INTERFACE,
                    }),
                };
                assert_eq!(problem, said, "{chip:?} {api} {families:?}");
            }
        }
    }
}
