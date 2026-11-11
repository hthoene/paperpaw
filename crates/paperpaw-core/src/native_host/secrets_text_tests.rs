//! Interface 6's secrets text on a device that seals: sealed lines opened into
//! veiled ones, veiled lines sealed, what does not open or is not understood
//! left as it is, a sealer that cannot seal failing the write.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
use super::*;
use core::cell::Cell;
use paperpaw_native::host_glue::ERR_STATE;
use paperpaw_sdk::secrets::{SecretChange, Secrets};

/// A stand-in for the device sealer: the key mixes into the value, the tag
/// covers the line's owner and name and the sealed bytes.
pub(crate) struct Fake {
    pub(crate) key: u8,
    /// Seals no value now (no radio noise for a nonce): `seal` is `ERR_STATE`.
    pub(crate) fails: Cell<bool>,
}

impl Fake {
    pub(crate) fn new(key: u8) -> Self {
        Self {
            key,
            fails: Cell::new(false),
        }
    }
    fn tag(&self, aad: &[u8], body: &[u8]) -> [u8; 16] {
        let mut tag = [self.key; 16];
        for (i, b) in aad.iter().chain(body).enumerate() {
            tag[i % 16] = tag[i % 16].rotate_left(3) ^ b ^ self.key;
        }
        tag
    }
}

impl Sealer for Fake {
    fn seals(&self) -> bool {
        true
    }
    fn seal(&self, aad: &[u8], plain: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        if self.fails.get() {
            return Err(ERR_STATE);
        }
        let total = 12 + plain.len() + 16;
        let out = out.get_mut(..total).ok_or(ERR_IO)?;
        out[..12].fill(7);
        for (i, b) in plain.iter().enumerate() {
            out[12 + i] = b ^ self.key ^ 7;
        }
        let tag = self.tag(aad, &out[..12 + plain.len()]);
        out[12 + plain.len()..].copy_from_slice(&tag);
        Ok(total)
    }
    fn open(&self, aad: &[u8], sealed: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        let body_len = sealed.len().checked_sub(28).ok_or(ERR_IO)?;
        let (body, tag) = sealed.split_at(12 + body_len);
        if self.tag(aad, body) != tag {
            return Err(ERR_IO);
        }
        let out = out.get_mut(..body_len).ok_or(ERR_IO)?;
        for (i, b) in body[12..].iter().enumerate() {
            out[i] = b ^ self.key ^ 7;
        }
        Ok(body_len)
    }
}

fn secrets() -> Secrets {
    let mut secrets = Secrets::default();
    secrets.apply(SecretChange::keep("wifi.Home\tNet", "Home", "pa$$word"));
    secrets.apply(SecretChange::keep("mail.pw", "Mail", "s3cr\u{e9}t"));
    secrets
}

#[test]
fn what_is_sealed_opens_back_into_the_list() {
    let sealer = Fake::new(9);
    let written = seal_text(&secrets().encode(), "", &sealer).unwrap();
    assert!(written.lines().all(|line| line.contains("\ts1:")));
    assert!(!written.contains("pa$$word"));
    assert_eq!(Secrets::decode(&open_text(&written, &sealer)), secrets());
}

#[test]
fn a_line_that_does_not_open_here_is_kept_as_it_is() {
    let other = Fake::new(3);
    let here = Fake::new(9);
    let foreign = seal_text(&secrets().encode(), "", &other).unwrap();
    let list = Secrets::decode(&open_text(&foreign, &here));
    assert!(list.is_empty(), "nothing opens under another key");
    // Written back, the lines are as they were: not lost, not resealed.
    assert_eq!(seal_text(&list.encode(), &foreign, &here).unwrap(), foreign);
}

#[test]
fn a_sealed_value_copied_to_another_name_does_not_open() {
    let sealer = Fake::new(9);
    let written = seal_text(&secrets().encode(), "", &sealer).unwrap();
    let moved = written.replace("mail.pw", "mail.other");
    let list = Secrets::decode(&open_text(&moved, &sealer));
    assert_eq!(list.get("mail.other"), None);
    assert_eq!(list.get("wifi.Home\tNet"), Some("pa$$word"));
}

#[test]
fn a_sealer_that_cannot_seal_fails_the_write_and_writes_nothing() {
    let sealer = Fake::new(9);
    sealer.fails.set(true);
    assert_eq!(seal_text(&secrets().encode(), "", &sealer), Err(ERR_STATE));
}

#[test]
fn lines_these_functions_do_not_understand_pass_through() {
    let sealer = Fake::new(9);
    let text = "no tabs here\nid\tlabel\tzz\t00\nwifi.a\tL\t\tnot-hex\n";
    assert_eq!(open_text(text, &sealer), text);
    assert_eq!(seal_text(text, "", &sealer).unwrap(), text);
}

#[test]
fn a_changeable_flag_and_an_escaped_label_are_kept() {
    let sealer = Fake::new(9);
    let mut list = Secrets::default();
    list.apply(SecretChange::keep("a.b", "La\tbel", "v").changeable(true));
    let written = seal_text(&list.encode(), "", &sealer).unwrap();
    assert_eq!(Secrets::decode(&open_text(&written, &sealer)), list);
}

#[test]
fn a_secret_whose_id_cannot_be_sealed_fails_the_write_and_is_never_veiled() {
    let sealer = Fake::new(9);
    let mut list = Secrets::default();
    // No `.`: no owner to bind the sealed value to.
    list.apply(SecretChange::keep("kosync", "Key", "abc"));
    assert_eq!(
        seal_text(&list.encode(), "", &sealer),
        Err(paperpaw_native::host_glue::ERR_INVALID)
    );
}

#[test]
fn lines_that_did_not_change_are_kept_and_need_no_new_seal() {
    let sealer = Fake::new(9);
    let before = seal_text(&secrets().encode(), "", &sealer).unwrap();
    // Removing one secret seals nothing: a sealer without a source is fine.
    sealer.fails.set(true);
    let mut list = Secrets::decode(&open_text(&before, &sealer));
    list.apply(SecretChange::remove("mail.pw"));
    let after = seal_text(&list.encode(), &before, &sealer).unwrap();
    assert_eq!(after.lines().count(), 1);
    assert!(before.contains(after.lines().next().unwrap()));
    // A changed value is sealed anew, and so fails here.
    list.apply(SecretChange::keep("wifi.Home\tNet", "Home", "other"));
    assert_eq!(seal_text(&list.encode(), &before, &sealer), Err(ERR_STATE));
}

#[test]
fn an_old_line_that_cannot_be_sealed_is_written_back_as_read_and_blocks_nothing() {
    let sealer = Fake::new(9);
    let mut list = Secrets::default();
    list.apply(SecretChange::keep("kosync", "Key", "abc"));
    let old = list.encode();
    // The file holds it (an old veiled line); a save of another secret
    // keeps it as it is and seals the new one.
    list.apply(SecretChange::keep("mail.pw", "Mail", "one"));
    let written = seal_text(&list.encode(), &old, &sealer).unwrap();
    assert!(written.contains(old.lines().next().unwrap()));
    assert!(written.lines().any(|line| line.contains("\ts1:")));
    // The same line where the file did not hold it is still refused.
    assert_eq!(
        seal_text(&list.encode(), "", &sealer),
        Err(paperpaw_native::host_glue::ERR_INVALID)
    );
}
