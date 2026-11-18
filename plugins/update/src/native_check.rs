//! What the device's check of an update file comes to for the page: who signed
//! it, whether it is newer, older or the same as the firmware running, whether
//! it is another firmware's image, and which file the page offers (the newest
//! that fits, as interface 6's `found::find` did). The device checks the file
//! and says the facts (`docs/abi/device.md` 5.1); the plugin never reads the
//! image or its trailer (decisions 132 and 134), it only reads the verdict.
use alloc::{string::String, vec::Vec};
use core::cmp::Ordering;
use paperpaw_sdk::{
    Version,
    native::{Error, device::DeviceOutcome},
};
use paperpaw_ui_kit::text::{concat, format};

/// Who signed an image the device would install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signer {
    /// A PaperPaw key.
    PaperPaw,
    /// Another key (the reader's own builds): installs only with this key, which
    /// the reader accepted after seeing its fingerprint.
    Own([u8; 32]),
}

/// A PaperPaw image the device would install, as far as it checked it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub path: String,
    pub version: String,
    pub signer: Signer,
    /// Its version against the one running: older and the same are the reader's
    /// choice, told so.
    pub order: Ordering,
    /// The plugins built in now that it leaves out, by their names, `, ` between
    /// (the device lists them after the check; empty where it leaves none out).
    pub leaves: String,
}

/// Another firmware's image: not PaperPaw, not signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Other {
    pub path: String,
    /// Its project and version as the device read them, or the path.
    pub name: String,
}

/// What one file of `updates` is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    Image(Image),
    Foreign(Other),
    /// Not installable, for this reason.
    Unfit(DeviceOutcome),
}

/// What the device's `check` record says of one file (`Device::check`).
#[derive(Debug, Clone, Copy)]
pub struct Evidence<'a> {
    pub reason: DeviceOutcome,
    /// Signed by a PaperPaw key.
    pub paperpaw: bool,
    /// Another firmware than PaperPaw.
    pub foreign: bool,
    /// The signing key, where there is one.
    pub key: Option<[u8; 32]>,
    pub id: &'a str,
    pub version: &'a str,
}

/// `a` against `b` as versions: a version of the SDK's is ordered by its
/// rules (`0.5.0-beta.2` before `0.5.0`), any other tag by its numbers and
/// letters as a file name sorts.
#[must_use]
pub fn compare(a: &str, b: &str) -> Ordering {
    match (Version::parse(a), Version::parse(b)) {
        (Some(a), Some(b)) => a.cmp(&b),
        _ => crate::native_card::natural(a, b),
    }
}

/// What the check of the file `path` comes to, with the firmware `running`.
///
/// A PaperPaw image the host would install, or would install only older, or
/// only with its key, is an [`Image`]; another firmware is [`Found::Foreign`]
/// (the host's verdict for one is that it is not PaperPaw's, which is no
/// failure here); anything else fails for the host's reason.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn judge(path: &str, evidence: &Evidence<'_>, running: &str) -> Result<Found, Error> {
    use DeviceOutcome as O;
    if evidence.foreign {
        return match evidence.reason {
            O::Ok | O::Unsigned | O::UnknownKey | O::NotNewer => {
                let named = concat(&[evidence.id, " ", evidence.version])?;
                let name = if named.trim().is_empty() {
                    path
                } else {
                    named.trim()
                };
                Ok(Found::Foreign(Other {
                    path: concat(&[path])?,
                    name: concat(&[name])?,
                }))
            }
            other => Ok(Found::Unfit(other)),
        };
    }
    let signer = match (evidence.paperpaw, evidence.key) {
        (true, _) => Some(Signer::PaperPaw),
        (false, Some(key)) => Some(Signer::Own(key)),
        (false, None) => None,
    };
    match (evidence.reason, signer) {
        // Fits; or fits but for being older or the same, or for the key the
        // reader has yet to accept: all things the page offers with its words.
        (O::Ok | O::NotNewer | O::UnknownKey, Some(signer)) => {
            let order = compare(evidence.version, running);
            // The host's word is that it is not newer, whatever the tags say.
            let order = match (evidence.reason, order) {
                (O::NotNewer, Ordering::Greater) => Ordering::Equal,
                (_, order) => order,
            };
            Ok(Found::Image(Image {
                path: concat(&[path])?,
                version: concat(&[evidence.version])?,
                signer,
                order,
                leaves: String::new(),
            }))
        }
        (O::Ok, None) => Ok(Found::Unfit(O::Unsigned)),
        (other, _) => Ok(Found::Unfit(other)),
    }
}

/// The file the page offers: the newest image (the first listed of equal
/// versions), else the first other firmware, else the last failure, else
/// none.
#[must_use]
pub fn choose(files: Vec<Found>) -> Option<Found> {
    let mut best: Option<Image> = None;
    let mut foreign = None;
    let mut failure = None;
    for file in files {
        match file {
            Found::Image(image) => {
                let newer = best
                    .as_ref()
                    .is_none_or(|best| compare(&image.version, &best.version).is_gt());
                if newer {
                    best = Some(image);
                }
            }
            Found::Foreign(other) => {
                if foreign.is_none() {
                    foreign = Some(other);
                }
            }
            Found::Unfit(why) => failure = Some(why),
        }
    }
    match (best, foreign, failure) {
        (Some(image), _, _) => Some(Found::Image(image)),
        (None, Some(other), _) => Some(Found::Foreign(other)),
        (None, None, Some(why)) => Some(Found::Unfit(why)),
        (None, None, None) => None,
    }
}

/// Whether the page offers `found` by itself at start: a newer image, whoever
/// signed it (interface 6's `is_newer`).
#[must_use]
pub fn is_offer(found: &Found) -> bool {
    matches!(found, Found::Image(image) if image.order.is_gt())
}

/// A key short enough to compare by eye: its first eight bytes in four groups,
/// kept together on one line (`3F2A 91C0 77D4 0B1E`).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn fingerprint(key: &[u8; 32]) -> Result<String, Error> {
    let [a, b, c, d, e, f, g, h, ..] = *key;
    format(format_args!(
        "{a:02X}{b:02X}\u{a0}{c:02X}{d:02X}\u{a0}{e:02X}{f:02X}\u{a0}{g:02X}{h:02X}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use DeviceOutcome as O;

    fn evidence(reason: O, version: &str) -> Evidence<'_> {
        Evidence {
            reason,
            paperpaw: true,
            foreign: false,
            key: Some([7; 32]),
            id: "paperpaw-xteink",
            version,
        }
    }

    fn image(found: &Found) -> &Image {
        match found {
            Found::Image(image) => image,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_newer_paperpaw_image_is_offered_plainly() {
        let found = judge("updates/a.bin", &evidence(O::Ok, "0.10.0"), "0.9.1").unwrap();
        let image = image(&found);
        assert_eq!(image.signer, Signer::PaperPaw);
        assert_eq!(image.order, Ordering::Greater);
        assert_eq!(image.path, "updates/a.bin");
        // What it leaves out is the device's list, read after the check.
        assert!(image.leaves.is_empty());
        assert!(is_offer(&found));
    }

    #[test]
    fn older_and_the_same_are_told_by_the_version_and_the_host() {
        let older = judge("a", &evidence(O::NotNewer, "0.9.0"), "0.10.0").unwrap();
        assert_eq!(image(&older).order, Ordering::Less);
        let same = judge("a", &evidence(O::NotNewer, "0.10.0"), "0.10.0").unwrap();
        assert_eq!(image(&same).order, Ordering::Equal);
        // The host says it is not newer: the tags do not outvote it.
        let odd = judge("a", &evidence(O::NotNewer, "x.y"), "0.1.0").unwrap();
        assert_eq!(image(&odd).order, Ordering::Equal);
        assert!(!is_offer(&older) && !is_offer(&same));
        // A beta comes before its release.
        let beta = judge("a", &evidence(O::Ok, "0.5.0-beta.2"), "0.5.0").unwrap();
        assert_eq!(image(&beta).order, Ordering::Less);
    }

    #[test]
    fn another_key_is_an_own_build_with_its_key() {
        let mut own = evidence(O::UnknownKey, "0.10.0");
        own.paperpaw = false;
        own.key = Some([9; 32]);
        let found = judge("a", &own, "0.9.0").unwrap();
        assert_eq!(image(&found).signer, Signer::Own([9; 32]));
        assert_eq!(image(&found).order, Ordering::Greater);
        // No key, no own build.
        own.key = None;
        assert_eq!(
            judge("a", &own, "0.9.0").unwrap(),
            Found::Unfit(O::UnknownKey)
        );
        let mut unsigned = evidence(O::Ok, "0.10.0");
        unsigned.paperpaw = false;
        unsigned.key = None;
        assert_eq!(
            judge("a", &unsigned, "0.9.0").unwrap(),
            Found::Unfit(O::Unsigned)
        );
    }

    #[test]
    fn another_firmware_is_named_by_its_project_and_version() {
        let mut other = evidence(O::Ok, "1.2.3");
        other.foreign = true;
        other.paperpaw = false;
        other.key = None;
        other.id = "crosspoint";
        let found = judge("firmware.bin", &other, "0.9.0").unwrap();
        assert_eq!(
            found,
            Found::Foreign(Other {
                path: "firmware.bin".into(),
                name: "crosspoint 1.2.3".into()
            })
        );
        other.id = "";
        other.version = "";
        let Found::Foreign(nameless) = judge("firmware.bin", &other, "0.9.0").unwrap() else {
            panic!()
        };
        assert_eq!(nameless.name, "firmware.bin");
        // One for another chip is no choice.
        other.reason = O::OtherChip;
        assert_eq!(
            judge("x", &other, "0.9.0").unwrap(),
            Found::Unfit(O::OtherChip)
        );
    }

    #[test]
    fn failures_keep_their_reason() {
        for reason in [
            O::Damaged,
            O::OtherChip,
            O::Revoked,
            O::NewerInterface,
            O::NoRoom,
        ] {
            assert_eq!(
                judge("a", &evidence(reason, "1.0.0"), "0.9.0").unwrap(),
                Found::Unfit(reason)
            );
        }
    }

    #[test]
    fn the_newest_fit_is_chosen_as_interface_6_did() {
        let mut files = Vec::new();
        for (path, version) in [
            ("a", "0.2.0"),
            ("b", "0.10.0"),
            ("c", "0.9.0"),
            ("d", "0.10.0"),
        ] {
            files.push(judge(path, &evidence(O::Ok, version), "0.1.0").unwrap());
        }
        files.push(Found::Unfit(O::OtherChip));
        let chosen = choose(files).unwrap();
        assert_eq!(image(&chosen).path, "b");
        // Another firmware only where no update fits.
        let foreign = Found::Foreign(Other {
            path: "f".into(),
            name: "x".into(),
        });
        let both = alloc::vec![
            foreign.clone(),
            judge("a", &evidence(O::Ok, "1.0.0"), "0.1.0").unwrap()
        ];
        assert_eq!(image(&choose(both).unwrap()).path, "a");
        let none = alloc::vec![
            Found::Unfit(O::Damaged),
            foreign.clone(),
            Found::Unfit(O::OtherChip)
        ];
        assert_eq!(choose(none), Some(foreign));
        assert_eq!(
            choose(alloc::vec![
                Found::Unfit(O::Damaged),
                Found::Unfit(O::OtherChip)
            ]),
            Some(Found::Unfit(O::OtherChip))
        );
        assert_eq!(choose(Vec::new()), None);
    }

    #[test]
    fn a_key_reads_in_four_groups_that_stay_on_one_line() {
        let mut key = [0; 32];
        key[..8].copy_from_slice(&[0x3f, 0x2a, 0x91, 0xc0, 0x77, 0xd4, 0x0b, 0x1e]);
        let text = fingerprint(&key).unwrap();
        assert_eq!(text, "3F2A\u{a0}91C0\u{a0}77D4\u{a0}0B1E");
        assert_eq!(
            text.replace('\u{a0}', " "),
            paperpaw_update::fingerprint(&key)
        );
    }
}
