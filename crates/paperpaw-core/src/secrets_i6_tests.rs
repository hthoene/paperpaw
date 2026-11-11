//! Interface 6's passwords on a device whose `core.secrets` seals: read
//! through the sealer once a native plugin is admitted, written sealed, a
//! native plugin's lines kept, no veiled write where sealing fails.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
use super::*;
use crate::native_host::{
    secrets_file::Sealer,
    secrets_service::{Rng, SecretsServices},
    secrets_text::{seal_text, tests::Fake},
};
use alloc::rc::Rc;
use paperpaw_platform::Storage;
use paperpaw_sdk::{
    Id, MemoryStorage,
    secrets::{FILE, SecretChange},
};

struct Host {
    storage: MemoryStorage,
    seal: Fake,
}

impl SecretsHost for Host {
    fn storage(&self) -> &dyn Storage {
        &self.storage
    }
    fn sealer(&self) -> &dyn Sealer {
        &self.seal
    }
    fn rng(&self) -> Option<&dyn Rng> {
        None
    }
}

/// The veiled text of the secrets as interface 6 writes them.
fn veiled(entries: &[(&str, &str)]) -> String {
    let mut list = Secrets::default();
    for (id, value) in entries {
        list.apply(SecretChange::keep(id, "label", value));
    }
    list.encode()
}

fn file(kernel: &Kernel) -> String {
    let data = paperpaw_sdk::files::load_for_update(kernel.shared.storage.as_ref(), FILE, 65_536)
        .unwrap()
        .unwrap_or_default();
    String::from_utf8(data).unwrap()
}

/// A kernel whose card holds `text` as the secrets file, `core.secrets` wired
/// (with `host`'s sealer) once a native plugin was admitted.
fn kernel(text: &str, wired: bool) -> (Kernel, Rc<Host>) {
    let mut kernel = Kernel::builder(crate::tests::DEVICE)
        .storage(MemoryStorage::new().with(FILE, text.as_bytes()))
        .build()
        .unwrap();
    let host = Rc::new(Host {
        storage: MemoryStorage::new(),
        seal: Fake::new(9),
    });
    if wired {
        kernel
            .shared
            .native_instances
            .test_insert(Id::new_static("probe"), None, true)
            .unwrap();
        let hosts = kernel
            .shared
            .native_instances
            .test_resources()
            .unwrap()
            .hosts();
        hosts
            .wire_secrets(SecretsServices::new(host.clone()))
            .unwrap();
    }
    (kernel, host)
}

#[test]
fn without_core_secrets_the_file_stays_veiled_as_ever() {
    let veiled_text = veiled(&[("mail.pw", "one")]);
    let (mut kernel, _) = kernel(&veiled_text, false);
    kernel.open_sealed_secrets();
    assert_eq!(kernel.secrets().get("mail.pw"), Some("one"));
    kernel.set_secret(SecretChange::keep("mail.pw", "label", "two"));
    assert_eq!(file(&kernel), veiled(&[("mail.pw", "two")]));
}

#[test]
fn sealed_values_are_read_once_the_services_are_wired_and_saved_sealed() {
    let host = Fake::new(9);
    let sealed = seal_text(&veiled(&[("mail.pw", "one")]), &host).unwrap();
    let (mut kernel, _) = kernel(&sealed, true);
    // The build read the file before the services were there.
    assert_eq!(kernel.secrets().get("mail.pw"), None);
    kernel.open_sealed_secrets();
    assert_eq!(kernel.secrets().get("mail.pw"), Some("one"));
    kernel.set_secret(SecretChange::keep("wifi.Home", "Home", "two"));
    let written = file(&kernel);
    assert!(written.lines().all(|line| line.contains("\ts1:")));
    assert!(!written.contains("one") && !written.contains("two"));
    assert_eq!(kernel.secrets().get("wifi.Home"), Some("two"));
    assert!(kernel.secrets().saved("wifi.Home"));
}

#[test]
fn a_line_a_native_plugin_wrote_is_kept_by_the_next_interface_6_save() {
    let host = Fake::new(9);
    let sealed = seal_text(&veiled(&[("mail.pw", "one")]), &host).unwrap();
    let (mut kernel, _) = kernel(&sealed, true);
    kernel.open_sealed_secrets();
    // A native plugin keeps a value by the file (as `core.secrets` does).
    let more = seal_text(&veiled(&[("native.token", "abc")]), &host).unwrap();
    let both = alloc::format!("{sealed}{more}");
    kernel.shared.storage.write(FILE, both.as_bytes()).unwrap();
    kernel.set_secret(SecretChange::keep("wifi.Home", "Home", "two"));
    assert_eq!(kernel.secrets().get("native.token"), Some("abc"));
    let reread = Secrets::decode(&crate::native_host::secrets_text::open_text(
        &file(&kernel),
        &host,
    ));
    assert_eq!(reread.get("native.token"), Some("abc"));
    assert_eq!(reread.get("mail.pw"), Some("one"));
    assert_eq!(reread.get("wifi.Home"), Some("two"));
}

#[test]
fn a_save_that_cannot_seal_writes_nothing_and_is_not_saved() {
    let host = Fake::new(9);
    let sealed = seal_text(&veiled(&[("mail.pw", "one")]), &host).unwrap();
    let (mut kernel, wired) = kernel(&sealed, true);
    kernel.open_sealed_secrets();
    // The sealer's key is gone for this write (a device without the radio's
    // noise): the file is not touched, no value is written veiled.
    wired.seal.fails.set(true);
    kernel.set_secret(SecretChange::keep("wifi.Home", "Home", "two"));
    assert_eq!(file(&kernel), sealed);
    assert!(!kernel.secrets().saved("wifi.Home"));
    // The change applies until the end.
    assert_eq!(kernel.secrets().get("wifi.Home"), Some("two"));
}
