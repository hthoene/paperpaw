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
        wire(&mut kernel, &host);
    }
    (kernel, host)
}

/// Wires `core.secrets` with `host`'s sealer, a native plugin admitted.
fn wire(kernel: &mut Kernel, host: &Rc<Host>) {
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
    let sealed = seal_text(&veiled(&[("mail.pw", "one")]), "", &host).unwrap();
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
    let sealed = seal_text(&veiled(&[("mail.pw", "one")]), "", &host).unwrap();
    let (mut kernel, _) = kernel(&sealed, true);
    kernel.open_sealed_secrets();
    // A native plugin keeps a value by the file (as `core.secrets` does).
    let more = seal_text(&veiled(&[("native.token", "abc")]), "", &host).unwrap();
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
    let sealed = seal_text(&veiled(&[("mail.pw", "one")]), "", &host).unwrap();
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

/// On the X4 nothing is sealed at the start (no radio, so no nonce): a secret
/// setting's value in `paperpaw.ini` stays there then, and moves to the
/// secrets as soon as a radio is on, not at some later start (#81).
#[test]
fn a_secret_setting_s_value_the_start_could_not_seal_moves_once_a_radio_is_on() {
    use paperpaw_sdk::{
        SettingExtra, SettingSpec,
        builtin::{CONFIG_FILE, groups::OTHER},
        wifi::WifiState,
    };
    static SECRET: SettingExtra = SettingExtra::new().secret();
    let ini = "[mail]\npassword = abc\nuser = me\n";
    let mut kernel = Kernel::builder(crate::tests::DEVICE)
        .storage(MemoryStorage::new().with(CONFIG_FILE, ini))
        .wifi(true)
        .build()
        .unwrap();
    let host = Rc::new(Host {
        storage: MemoryStorage::new(),
        seal: Fake::new(9),
    });
    wire(&mut kernel, &host);
    host.seal.fails.set(true);
    // Mail registers once the file was read, as every native plugin.
    let password = SettingSpec::text("mail.password", "mail.password", "", OTHER);
    kernel
        .shared
        .settings
        .define(password.with_extra(&SECRET))
        .unwrap();
    kernel.apply_late_config();
    let config = |kernel: &Kernel| {
        let file = paperpaw_sdk::read_file(kernel.storage(), CONFIG_FILE, 4096).unwrap();
        String::from_utf8(file).unwrap()
    };
    assert_eq!(config(&kernel), ini);
    assert_eq!(file(&kernel), "");
    // Wi-Fi comes on: the value is sealed now and out of the file.
    host.seal.fails.set(false);
    kernel.wifi_event(
        paperpaw_platform::Instant(1),
        crate::WifiEvent::State(WifiState::On),
    );
    let (left, sealed) = (config(&kernel), file(&kernel));
    assert!(
        !left.contains("abc") && left.contains("user = me"),
        "{left}"
    );
    assert!(!sealed.contains("abc"), "{sealed}");
    let sealer: Rc<dyn SecretsHost> = host.clone();
    let (kept, unopened) = Kernel::decode_secrets(&sealed, Some(&sealer));
    assert!(unopened.is_empty());
    assert_eq!(kept.get("mail.password"), Some("abc"));
    // Nothing is left to move: the next change of the radio reads no file.
    assert!(!kernel.secrets_left);
}

#[test]
fn removing_a_password_needs_no_seal_so_it_works_without_the_radio() {
    let host = Fake::new(9);
    let sealed = seal_text(
        &veiled(&[("mail.pw", "one"), ("wifi.Home", "two")]),
        "",
        &host,
    )
    .unwrap();
    let (mut kernel, wired) = kernel(&sealed, true);
    kernel.open_sealed_secrets();
    wired.seal.fails.set(true);
    kernel.set_secret(SecretChange::remove("mail.pw"));
    assert!(kernel.secrets().saved("mail.pw"));
    let left = file(&kernel);
    assert_eq!(left.lines().count(), 1);
    assert!(sealed.contains(left.lines().next().unwrap()));
}

#[test]
fn a_start_reads_the_sealed_values_before_any_plugin_runs() {
    let host = Fake::new(9);
    let sealed = seal_text(&veiled(&[("mail.pw", "one")]), "", &host).unwrap();
    let (mut kernel, _) = kernel(&sealed, true);
    assert_eq!(kernel.secrets().get("mail.pw"), None);
    kernel.start(paperpaw_platform::Instant(0));
    assert_eq!(kernel.secrets().get("mail.pw"), Some("one"));
}

#[test]
fn a_secret_that_cannot_be_sealed_is_refused_at_save_and_never_applied() {
    let host = Fake::new(9);
    let sealed = seal_text(&veiled(&[("mail.pw", "one")]), "", &host).unwrap();
    let (mut kernel, _) = kernel(&sealed, true);
    kernel.open_sealed_secrets();
    kernel.set_secret(SecretChange::keep("kosync", "Key", "abc"));
    assert_eq!(kernel.secrets().get("kosync"), None);
    assert!(!kernel.secrets().saved("kosync"));
    assert_eq!(file(&kernel), sealed);
    // Later saves are not stopped by it.
    kernel.set_secret(SecretChange::keep("wifi.Home", "Home", "two"));
    assert!(kernel.secrets().saved("wifi.Home"));
    assert!(!file(&kernel).contains("kosync"));
}

#[test]
fn an_old_unsealable_line_in_the_file_does_not_stop_later_saves() {
    let host = Fake::new(9);
    let sealed = seal_text(&veiled(&[("mail.pw", "one")]), "", &host).unwrap();
    let old = veiled(&[("kosync", "abc")]);
    let text = alloc::format!("{sealed}{old}");
    let (mut kernel, _) = kernel(&text, true);
    kernel.open_sealed_secrets();
    kernel.set_secret(SecretChange::keep("wifi.Home", "Home", "two"));
    assert!(kernel.secrets().saved("wifi.Home"));
    let written = file(&kernel);
    assert!(written.contains(old.lines().next().unwrap()));
    assert!(written.contains("wifi.Home") && !written.contains("two"));
}

#[test]
fn a_shell_that_wired_core_secrets_seals_with_no_native_plugin_admitted() {
    let host = Fake::new(9);
    let sealed = seal_text(&veiled(&[("mail.pw", "one")]), "", &host).unwrap();
    // No native plugin, no pool: only the shell's wiring.
    let (mut kernel, wired) = kernel(&sealed, false);
    assert!(kernel.secrets_host().is_none());
    let lifecycle = crate::native_host::shell_wiring::SecretsLifecycle::new(wired.clone());
    kernel.wire_secrets(&lifecycle);
    assert_eq!(kernel.secrets().get("mail.pw"), Some("one"));
    kernel.set_secret(SecretChange::keep("wifi.Home", "Home", "two"));
    let written = file(&kernel);
    assert!(written.lines().all(|line| line.contains("\ts1:")));
    assert!(!written.contains("two"));
    // A sealer that cannot seal now: nothing is written veiled.
    wired.seal.fails.set(true);
    kernel.set_secret(SecretChange::keep("wifi.Work", "Work", "three"));
    assert!(!kernel.secrets().saved("wifi.Work"));
    assert_eq!(file(&kernel), written);
}

#[test]
fn a_card_from_another_device_is_told_once_per_start() {
    use crate::native_host::shell_wiring::{SecretsInputs, SecretsLifecycle};
    let theirs = seal_text(&veiled(&[("mail.pw", "one")]), "", &Fake::new(3)).unwrap();
    let mut kernel = Kernel::builder(crate::tests::DEVICE)
        .storage(MemoryStorage::new().with(FILE, theirs.as_bytes()))
        .items(
            crate::tests::notice_plugin_manifest(),
            crate::tests::notice_plugin(crate::tests::NoticePlugin {
                log: crate::tests::log(),
            }),
        )
        .build()
        .unwrap();
    kernel.start(paperpaw_platform::Instant(0));
    // No native plugin runs: the shell wired `core.secrets` only.
    let mine: Rc<dyn Storage> = Rc::new(MemoryStorage::new());
    let lifecycle = SecretsLifecycle::new(SecretsInputs::new(mine, Rc::new(Fake::new(9))).host());
    kernel.wire_secrets(&lifecycle);
    assert_eq!(kernel.secrets().get("mail.pw"), None, "absent here");
    assert!(kernel.notice.is_none());
    kernel.finish();
    assert!(kernel.notice.is_some(), "told");
    kernel.tick(paperpaw_platform::Instant(10_000));
    assert!(kernel.notice.is_none(), "the notice went by");
    // Read again by a save: not told twice in one start; the line stays.
    kernel.set_secret(SecretChange::keep("wifi.Home", "Home", "two"));
    kernel.tick(paperpaw_platform::Instant(11_000));
    assert!(kernel.notice.is_none());
    assert!(file(&kernel).contains(theirs.lines().next().unwrap()));
}

/// A kernel with interface 6's notice plugin whose card holds `text`, the
/// shell's `core.secrets` wired with `sealer` (no native plugin runs).
fn told_kernel(text: &str, sealer: Fake) -> Kernel {
    started(MemoryStorage::new().with(FILE, text.as_bytes()), sealer)
}

/// [`told_kernel`] on the card `card`.
fn started(card: MemoryStorage, sealer: Fake) -> Kernel {
    use crate::native_host::shell_wiring::{SecretsInputs, SecretsLifecycle};
    let mut kernel = Kernel::builder(crate::tests::DEVICE)
        .storage(card)
        .items(
            crate::tests::notice_plugin_manifest(),
            crate::tests::notice_plugin(crate::tests::NoticePlugin {
                log: crate::tests::log(),
            }),
        )
        .build()
        .unwrap();
    kernel.start(paperpaw_platform::Instant(0));
    let mine: Rc<dyn Storage> = Rc::new(MemoryStorage::new());
    let lifecycle = SecretsLifecycle::new(SecretsInputs::new(mine, Rc::new(sealer)).host());
    kernel.wire_secrets(&lifecycle);
    kernel
}

/// Senior R1 (#254): a busy or unreadable flash is not "another device".
#[test]
fn a_busy_flash_is_not_told_as_another_device() {
    let sealer = Fake::new(9);
    let mine = seal_text(&veiled(&[("mail.pw", "one")]), "", &sealer).unwrap();
    for fault in [
        paperpaw_native::host_glue::ERR_BUSY,
        paperpaw_native::host_glue::ERR_IO,
    ] {
        let busy = Fake::new(9);
        busy.open_fault.set(Some(fault));
        let mut kernel = told_kernel(&mine, busy);
        assert_eq!(kernel.secrets().get("mail.pw"), None, "absent for now");
        kernel.finish();
        assert!(kernel.notice.is_none(), "not told: the values are ours");
        assert!(file(&kernel).contains(mine.lines().next().unwrap()), "kept");
    }
}

/// CR (#2491): the same key saved anew over a line that did not open replaces
/// it: one line for it, sealed here, and the new value reads back.
#[test]
fn the_same_key_saved_anew_replaces_a_line_that_did_not_open() {
    let theirs = seal_text(&veiled(&[("wifi.Home", "one")]), "", &Fake::new(3)).unwrap();
    let mut kernel = told_kernel(&theirs, Fake::new(9));
    assert_eq!(kernel.secrets().get("wifi.Home"), None, "absent here");
    kernel.set_secret(SecretChange::keep("wifi.Home", "Home", "two"));
    assert!(kernel.secrets().saved("wifi.Home"));
    let written = file(&kernel);
    assert!(
        !written.contains(theirs.lines().next().unwrap()),
        "replaced"
    );
    assert_eq!(
        written
            .lines()
            .filter(|l| l.starts_with("wifi.Home\t"))
            .count(),
        1
    );
    let reread = Secrets::decode(&crate::native_host::secrets_text::open_text(
        &written,
        &Fake::new(9),
    ));
    assert_eq!(reread.get("wifi.Home"), Some("two"));
}

/// #466: what does not open is told once, not at every start; a line that did
/// not open and was not told yet is told again, once; one entered again (the
/// set grows smaller) is not.
#[test]
fn values_that_do_not_open_are_told_once_and_again_only_for_a_new_one() {
    use super::TOLD_FILE;
    // The card's file and what the start told, after one start on `card`.
    let start = |card: MemoryStorage| {
        let mut kernel = started(card, Fake::new(9));
        kernel.finish();
        let told = kernel.notice.is_some();
        kernel.tick(paperpaw_platform::Instant(10_000));
        let storage = kernel.shared.storage.as_ref();
        let read = |path| paperpaw_sdk::files::load_for_update(storage, path, 65_536).unwrap();
        (told, read(FILE).unwrap(), read(TOLD_FILE))
    };
    let card = |secrets: &[u8], told: Option<&[u8]>| {
        let card = MemoryStorage::new().with(FILE, secrets);
        match told {
            Some(told) => card.with(TOLD_FILE, told),
            None => card,
        }
    };
    let theirs = seal_text(&veiled(&[("mail.pw", "one")]), "", &Fake::new(3)).unwrap();
    let (told, secrets, mark) = start(card(theirs.as_bytes(), None));
    assert!(told, "first start: told");
    let mark = mark.expect("the told lines are kept");
    // The next start on the same card says nothing and writes nothing.
    let (told, _, again) = start(card(&secrets, Some(&mark)));
    assert!(!told, "second start: not told again");
    assert_eq!(again.as_deref(), Some(&*mark));
    // Another device's line comes to the card: told once more.
    let more = seal_text(&veiled(&[("wifi.Home", "two")]), "", &Fake::new(4)).unwrap();
    let mut grown = secrets.clone();
    grown.extend_from_slice(more.as_bytes());
    let (told, _, mark) = start(card(&grown, Some(&mark)));
    assert!(told, "a new line that does not open: told");
    let mark = mark.unwrap();
    let (told, _, _) = start(card(&grown, Some(&mark)));
    assert!(!told, "and only once");
    // One entered again here: the rest was told, so nothing is said.
    let mut kernel = started(card(&grown, Some(&mark)), Fake::new(9));
    kernel.set_secret(SecretChange::keep("wifi.Home", "Home", "three"));
    let entered = file(&kernel).into_bytes();
    let (told, _, _) = start(card(&entered, Some(&mark)));
    assert!(!told, "fewer lines that do not open: not told");
}
