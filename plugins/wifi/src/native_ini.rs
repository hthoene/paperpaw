//! The `wifi.ini` on the card on interface 7 (slice 5): networks put in the
//! plugin's data folder (e.g. by the flasher), read once at a start, kept
//! among the plugin's secrets and then removed. The rules of the
//! interface-6 plugin (`wifi_file_networks`), host-free where they can be,
//! with fallible allocation; the passwords read are wiped.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::storage::{self, MAX_DATA, Root};
use paperpaw_sdk::native::{Error, Host};
use paperpaw_ui_kit::{
    mem::{buffer, push, wipe},
    text::copy,
};

use crate::native_known::{MAX_KNOWN, MAX_PASSWORD, MAX_SSID};
use crate::native_policy::Password;

/// The name of the file, in the plugin's data folder.
pub const FILE: &str = "wifi.ini";
/// The largest file read; a larger one is removed, unread.
pub const MOST_BYTES: usize = 16 * 1024;

/// A network of the file, with its password (empty for an open one).
pub struct FileNetwork {
    pub ssid: String,
    pub password: Password,
}

impl core::fmt::Debug for FileNetwork {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FileNetwork")
            .field("ssid", &self.ssid)
            .finish_non_exhaustive()
    }
}

/// What the file held.
#[derive(Debug)]
#[non_exhaustive]
pub enum Content {
    /// There is no file (or it cannot be read).
    Nothing,
    /// The file is larger than [`MOST_BYTES`]: it was removed, unread.
    TooLarge,
    /// The networks in it.
    Networks(Vec<FileNetwork>),
}

/// The networks of a file: lines `ssid = …` and `password = …`, a password
/// going with the `ssid` line before it (the last one counts). Lines that are
/// not text are skipped; at most [`MAX_KNOWN`] networks are taken (no more can
/// be kept). A name over [`MAX_SSID`] bytes or a password over
/// [`MAX_PASSWORD`] cannot belong to a network: it is skipped before it is
/// copied (with its network), so that a large file costs no large copies.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn networks(bytes: &[u8]) -> Result<Vec<FileNetwork>, Error> {
    let mut networks: Vec<FileNetwork> = Vec::new();
    // The name of the last ssid line was too long: its passwords go nowhere.
    let mut dropped = false;
    // The last password of the last network was too long (a later one may
    // still be right, as the last password counts).
    let mut too_long = false;
    for line in bytes.split(|byte| *byte == b'\n') {
        let Ok(line) = core::str::from_utf8(line) else {
            continue;
        };
        let Some((key, value)) = line.trim().split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "ssid" if !value.is_empty() => {
                if networks.len() == MAX_KNOWN {
                    break;
                }
                if too_long {
                    networks.pop();
                    too_long = false;
                }
                dropped = value.len() > MAX_SSID;
                if dropped {
                    continue;
                }
                push(
                    &mut networks,
                    FileNetwork {
                        ssid: copy(value)?,
                        password: Password::new("")?,
                    },
                )?;
            }
            "password" if !dropped => {
                if value.len() > MAX_PASSWORD {
                    // Not copied; the network cannot be kept with it unless a
                    // later password replaces it.
                    too_long = true;
                } else if let Some(network) = networks.last_mut() {
                    network.password = Password::new(value)?;
                    too_long = false;
                }
            }
            _ => {}
        }
    }
    if too_long {
        networks.pop();
    }
    Ok(networks)
}

/// Reads the file. A file larger than [`MOST_BYTES`] is removed so that no
/// password stays on the card; a file that cannot be read is left for the
/// next start.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn read(host: &Host<'_>) -> Result<Content, Error> {
    let size = match storage::size(host, Root::Data, FILE) {
        Ok(size) => size,
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => return Ok(Content::Nothing),
    };
    // A size beyond the machine's words is too large as well.
    let size = usize::try_from(size).unwrap_or(usize::MAX);
    if size > MOST_BYTES {
        let _ = storage::remove(host, Root::Data, FILE);
        return Ok(Content::TooLarge);
    }
    let mut bytes = buffer(size)?;
    let mut at = 0;
    while at < size {
        let end = size.min(at + MAX_DATA);
        match storage::read_at(host, Root::Data, FILE, at as u64, &mut bytes[at..end]) {
            Ok(0) | Err(_) => {
                wipe(&mut bytes);
                return Ok(Content::Nothing);
            }
            Ok(read) => at += read,
        }
    }
    let found = networks(&bytes);
    wipe(&mut bytes);
    found.map(Content::Networks)
}

/// Removes the file (its networks are kept).
pub fn remove(host: &Host<'_>) {
    let _ = storage::remove(host, Root::Data, FILE);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::string::ToString;

    fn pairs(bytes: &[u8]) -> Vec<(String, String)> {
        networks(bytes)
            .unwrap()
            .iter()
            .map(|n| (n.ssid.clone(), n.password.as_str().to_string()))
            .collect()
    }

    #[test]
    fn a_password_goes_with_the_ssid_before_it() {
        let text =
            "ssid = Home\npassword = secret\n\nssid=Cafe\r\n; note\nssid =\npassword = lost\n";
        assert_eq!(
            pairs(text.as_bytes()),
            [
                ("Home".to_string(), "secret".to_string()),
                ("Cafe".to_string(), "lost".to_string())
            ]
        );
        // Before any ssid a password goes nowhere; the last password counts.
        assert_eq!(
            pairs(b"password = x\nssid = A\npassword = 1\npassword = 2\n"),
            [("A".to_string(), "2".to_string())]
        );
    }

    /// What the interface-6 plugin read of these (fixed when its code went).
    #[test]
    fn it_reads_what_the_interface_6_plugin_read() {
        let samples: [(&str, &[(&str, &str)]); 8] = [
            ("", &[]),
            ("ssid = A", &[("A", "")]),
            (
                "ssid = A\npassword = p\nssid = B\n",
                &[("A", "p"), ("B", "")],
            ),
            ("  ssid  =  A b  \n password=  p q \n", &[("A b", "p q")]),
            ("ssid = A = B\npassword = x = y\n", &[("A = B", "x = y")]),
            (
                "ssid = Ünï\npassword = pässword\r\nssid = C\r\n",
                &[("Ünï", "pässword"), ("C", "")],
            ),
            ("junk\nssid\npassword\n=\nssid = D\n", &[("D", "")]),
            ("SSID = A\nssid = E\nPassword = no\n", &[("E", "")]),
        ];
        for (text, expected) in samples {
            let expected: Vec<(String, String)> = expected
                .iter()
                .map(|(ssid, password)| ((*ssid).to_string(), (*password).to_string()))
                .collect();
            assert_eq!(pairs(text.as_bytes()), expected, "{text:?}");
        }
    }

    #[test]
    fn a_line_that_is_not_text_is_skipped() {
        let mut bytes = b"ssid = A\npassword = ".to_vec();
        bytes.extend_from_slice(&[0xff, 0xfe]);
        bytes.extend_from_slice(b"\nssid = B\n");
        assert_eq!(
            pairs(&bytes),
            [
                ("A".to_string(), String::new()),
                ("B".to_string(), String::new())
            ]
        );
    }

    #[test]
    fn a_name_or_a_password_too_long_for_a_network_is_skipped_with_its_network() {
        let long_name = "n".repeat(MAX_SSID + 1);
        let long_pass = "p".repeat(MAX_PASSWORD + 1);
        let exact_name = "n".repeat(MAX_SSID);
        let exact_pass = "p".repeat(MAX_PASSWORD);
        let text = std::format!(
            "ssid = {long_name}\npassword = x\nssid = A\npassword = {long_pass}\npassword = y\n\
             ssid = B\npassword = z\nssid = {exact_name}\npassword = {exact_pass}\n"
        );
        // A password too long that is the last of its network drops it.
        assert_eq!(
            pairs(std::format!("ssid = C\npassword = {long_pass}\nssid = D\n").as_bytes()),
            [("D".to_string(), String::new())]
        );
        assert_eq!(
            pairs(text.as_bytes()),
            [
                // The last password counts: the long one before it is gone.
                ("A".to_string(), "y".to_string()),
                ("B".to_string(), "z".to_string()),
                (exact_name, exact_pass)
            ]
        );
    }

    #[test]
    fn no_more_than_can_be_kept_are_taken() {
        let mut text = String::new();
        for n in 0..MAX_KNOWN + 5 {
            text.push_str(&std::format!("ssid = n{n}\npassword = p\n"));
        }
        assert_eq!(networks(text.as_bytes()).unwrap().len(), MAX_KNOWN);
    }
}
