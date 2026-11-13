//! The firmware commands in the DEV stage: checked whole, accepted at submit,
//! given back when the batch never applies.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::super::{
    DevBatchOutput, DevContext, DevHost, FEATURE_WIFI, InputClock, NoStoredNetworks,
};
use super::*;
use crate::{
    Kernel,
    abi7::commands_wire::{commands_decode, commands_encode},
    native_host::{
        admitted_features::AdmittedFeatures,
        firmware_driver::Driver,
        firmware_install::{BuiltIns, Event, FirmwareInstall},
    },
};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_FULL, ERR_STATE, OK};
use paperpaw_native::metadata::Feature;
use paperpaw_platform::{
    Instant, NoStorage,
    firmware::WRITE_BLOCK,
    firmware_fake::{FakeFirmware, paperpaw_facts},
};
use paperpaw_sdk::Id;

const HOST: &[Feature<'static>] = &[
    Feature {
        key: FEATURE_FIRMWARE,
        revision: 1,
    },
    Feature {
        key: FEATURE_INSTALL,
        revision: 1,
    },
    Feature {
        key: "uses.storage",
        revision: 1,
    },
    Feature {
        key: FEATURE_WIFI,
        revision: 1,
    },
];
const FIRMWARE: u32 = 1;
const INSTALL: u32 = 2;
const STORAGE: u32 = 4;
const WIFI: u32 = 8;
const OWNER: u64 = 9;

struct Names;
impl BuiltIns for Names {
    fn count(&self) -> usize {
        0
    }
    fn index_of(&self, _: &str) -> Option<usize> {
        None
    }
}

struct Clock;
impl InputClock for Clock {
    fn now_ms(&self) -> u64 {
        0
    }
    fn last_input_ms(&self) -> Option<u64> {
        None
    }
}

fn driver() -> DriverRef {
    let fake = FakeFirmware::new(
        "0.5.0",
        "paperpaw-x4",
        5,
        paperpaw_facts("0.6.0", "paperpaw-x4", 2 * WRITE_BLOCK),
    );
    crate::native_host::firmware_cell::test_shared(RefCell::new(Driver::new(
        alloc::boxed::Box::new(fake),
        crate::native_host::firmware_cell::test_shared(RefCell::new(FirmwareInstall::new())),
    )))
}

fn kernel() -> Kernel {
    Kernel::builder(crate::tests::DEVICE).build().unwrap()
}

fn install(request: u32, path: &str) -> WireCommand<'_> {
    WireCommand::InstallFirmware {
        request,
        root: 0,
        path,
        key: &[],
        flags: 0,
    }
}

fn check(request: u32, kind: u32) -> WireCommand<'static> {
    WireCommand::CheckFile {
        request,
        root: 0,
        path: "update.ppu",
        kind,
    }
}

/// Stages `commands` for the plugin `OWNER` with `bits` of the features and
/// `firmware` as the device's table (none: no firmware host); the status and
/// the output, undrained.
fn stage(
    kernel: &Kernel,
    bits: u32,
    firmware: Option<&DriverRef>,
    commands: &[WireCommand<'_>],
) -> (i32, DevBatchOutput) {
    let id = Id::new_static("update");
    let env = kernel.shared.env(Instant(0));
    let mut ctx = DevContext::new(AdmittedFeatures::test(bits, HOST), &kernel.radio_requests);
    if let Some(driver) = firmware {
        let cell =
            crate::native_host::firmware_cell::FirmwareCell::running(driver.try_clone().unwrap());
        ctx = ctx.with_firmware(FirmwareContext::new(Rc::new(cell), OWNER));
    }
    let mut stage =
        crate::native_host::commands::CommandStage::new(&id, env.settings(), None, 16, 4096)
            .with_dev(ctx);
    let mut bytes = [0; 2048];
    let len = commands_encode(commands, &mut bytes).unwrap();
    let status = stage.stage(&commands_decode(&bytes[..len]).unwrap());
    (status, stage.into_dev_output())
}

fn drain(kernel: &mut Kernel, output: DevBatchOutput) {
    let (staged, redraw, _, batch) = output;
    let host = DevHost {
        clock: &Clock,
        stored: &NoStoredNetworks,
    };
    kernel.drain_native_dev(&Id::new_static("update"), staged, redraw, batch, &host);
}

#[test]
fn without_a_firmware_host_or_a_feature_the_commands_are_refused() {
    let kernel = kernel();
    let d = driver();
    let all = FIRMWARE | INSTALL | STORAGE;
    // No host: unsupported. No feature admitted: denied.
    assert_eq!(
        stage(&kernel, all, None, &[install(1, "a.ppu")]).0,
        ERR_UNSUPPORTED
    );
    assert_eq!(
        stage(&kernel, 0, Some(&d), &[install(1, "a.ppu")]).0,
        ERR_DENIED
    );
    assert_eq!(
        stage(&kernel, 0, Some(&d), &[WireCommand::FirmwareRestart {}]).0,
        ERR_DENIED
    );
    assert_eq!(d.borrow().waiting(), 0);
}

#[test]
fn the_fields_are_checked_before_anything_is_accepted() {
    let kernel = kernel();
    let d = driver();
    let all = FIRMWARE | INSTALL | STORAGE;
    let key5 = [1_u8; 5];
    let bad_key = WireCommand::InstallFirmware {
        request: 1,
        root: 0,
        path: "a.ppu",
        key: &key5,
        flags: 0,
    };
    let bad_flags = WireCommand::InstallFirmware {
        request: 1,
        root: 0,
        path: "a.ppu",
        key: &[],
        flags: 4,
    };
    let bad_root = WireCommand::InstallFirmware {
        request: 1,
        root: 2,
        path: "a.ppu",
        key: &[],
        flags: 0,
    };
    for command in [bad_key, bad_flags, bad_root, install(0, "a.ppu")] {
        assert_eq!(stage(&kernel, all, Some(&d), &[command]).0, ERR_INVALID);
    }
    assert_eq!(
        stage(&kernel, all, Some(&d), &[install(1, "")]).0,
        ERR_INVALID
    );
    // The user's files (root 1) need `uses.storage`.
    let card = WireCommand::InstallFirmware {
        request: 1,
        root: 1,
        path: "a.ppu",
        key: &[],
        flags: 0,
    };
    assert_eq!(stage(&kernel, FIRMWARE, Some(&d), &[card]).0, ERR_DENIED);
    // A plugin check needs a plugin host (the driver has none here); a kind
    // beyond 1 is denied.
    assert_eq!(
        stage(&kernel, all, Some(&d), &[check(1, 0)]).0,
        ERR_UNSUPPORTED
    );
    assert_eq!(stage(&kernel, all, Some(&d), &[check(1, 2)]).0, ERR_DENIED);
    assert_eq!(d.borrow().waiting(), 0, "a refusal accepts nothing");
}

/// A driver that accepts plugin jobs.
fn plugin_driver() -> DriverRef {
    use crate::native_host::plugin_job::PluginHost;
    use paperpaw_native::{
        header5::{Host as Loader, X4_PROFILE},
        metadata::Host as Offered,
    };
    let d = driver();
    d.borrow_mut().set_plugin_host(PluginHost {
        loader: Loader::x4(1),
        offered: Offered {
            abi_major: 7,
            minor: 1,
            profile: X4_PROFILE,
            services: &[],
            features: &[],
        },
        signers: &[],
    });
    d
}

#[test]
fn a_plugin_check_is_a_job_of_the_queue_for_uses_install_with_a_plugin_host() {
    let mut kernel = kernel();
    let d = plugin_driver();
    // `uses.install` admits kind 0, `uses.firmware` kind 1: not each other's.
    assert_eq!(
        stage(&kernel, FIRMWARE, Some(&d), &[check(1, 0)]).0,
        ERR_DENIED
    );
    assert_eq!(
        stage(&kernel, INSTALL, Some(&d), &[check(1, 1)]).0,
        ERR_DENIED
    );
    // Request 0 and a root that is not the plugin's may not be read.
    assert_eq!(
        stage(&kernel, INSTALL, Some(&d), &[check(0, 0)]).0,
        ERR_INVALID
    );
    let card = WireCommand::CheckFile {
        request: 1,
        root: 1,
        path: "Download/dials",
        kind: 0,
    };
    assert_eq!(stage(&kernel, INSTALL, Some(&d), &[card]).0, ERR_DENIED);
    assert_eq!(d.borrow().waiting(), 0, "a refusal accepts nothing");
    // Accepted at submit, taken back when the batch never applies.
    let (status, output) = stage(&kernel, INSTALL, Some(&d), &[check(1, 0)]);
    assert_eq!(status, OK);
    assert!(d.borrow().holds(OWNER, 1));
    drop(output);
    assert_eq!(d.borrow().waiting(), 0);
    // A drained one is kept, and takes a place of the shared table.
    let (status, output) = stage(&kernel, INSTALL, Some(&d), &[check(2, 0)]);
    assert_eq!(status, OK);
    drain(&mut kernel, output);
    assert!(d.borrow().holds(OWNER, 2));
    let (status, output) = stage(&kernel, INSTALL | FIRMWARE, Some(&d), &[check(3, 1)]);
    assert_eq!(status, OK);
    drain(&mut kernel, output);
    assert_eq!(
        stage(&kernel, INSTALL, Some(&d), &[check(4, 0)]).0,
        ERR_FULL,
        "two open requests per plugin, plugin jobs and firmware ones alike"
    );
}

#[test]
fn one_firmware_command_a_batch() {
    let kernel = kernel();
    let d = driver();
    let (status, _) = stage(
        &kernel,
        FIRMWARE,
        Some(&d),
        &[install(1, "a.ppu"), check(2, 1)],
    );
    assert_eq!(status, ERR_INVALID);
    assert_eq!(d.borrow().waiting(), 0);
}

#[test]
fn an_accepted_request_waits_and_a_batch_that_never_applies_gives_it_back() {
    let mut kernel = kernel();
    let d = driver();
    let (status, output) = stage(&kernel, FIRMWARE, Some(&d), &[install(1, "a.ppu")]);
    assert_eq!(status, OK);
    assert!(d.borrow().holds(OWNER, 1), "accepted at submit");
    drop(output);
    assert_eq!(d.borrow().waiting(), 0, "an undrained output takes it back");
    // A drained one keeps it.
    let (status, output) = stage(&kernel, FIRMWARE, Some(&d), &[check(2, 1)]);
    assert_eq!(status, OK);
    drain(&mut kernel, output);
    assert!(d.borrow().holds(OWNER, 2));
    // The same number again is refused while it waits.
    assert_eq!(
        stage(&kernel, FIRMWARE, Some(&d), &[check(2, 1)]).0,
        ERR_INVALID
    );
}

#[test]
fn a_release_that_finds_the_table_borrowed_is_counted() {
    let kernel = kernel();
    let d = driver();
    let (status, output) = stage(&kernel, FIRMWARE, Some(&d), &[install(1, "a.ppu")]);
    assert_eq!(status, OK);
    let held = d.borrow_mut();
    drop(output);
    drop(held);
    assert_eq!(
        d.borrow().skipped(),
        1,
        "the request stays, and it is counted"
    );
}

#[test]
fn the_third_request_of_one_plugin_is_full_and_nothing_of_it_is_kept() {
    let kernel = kernel();
    let d = driver();
    let mut kept = Vec::new();
    for request in 1..=2 {
        let (status, output) = stage(&kernel, FIRMWARE, Some(&d), &[install(request, "a.ppu")]);
        assert_eq!(status, OK);
        kept.push(output);
    }
    // Two open requests per plugin (device-wide stays four).
    let (status, output) = stage(&kernel, FIRMWARE, Some(&d), &[install(5, "a.ppu")]);
    assert_eq!(status, ERR_FULL);
    drop(output);
    assert_eq!(d.borrow().waiting(), 2);
    assert!(!d.borrow().holds(OWNER, 5));
}

#[test]
fn a_restart_is_for_the_plugin_whose_install_waits_and_the_host_then_asks() {
    let mut kernel = kernel();
    let d = driver();
    // None waiting: `ERR_STATE`.
    assert_eq!(
        stage(
            &kernel,
            FIRMWARE,
            Some(&d),
            &[WireCommand::FirmwareRestart {}]
        )
        .0,
        ERR_STATE
    );
    // The owner's install runs to state 3.
    d.borrow_mut()
        .submit_install(OWNER, 1, "a.ppu", None, 0)
        .unwrap();
    let ask = loop {
        if let Event::Ask(ask) = d.borrow_mut().pump(&NoStorage, &Names, 100) {
            break ask;
        }
    };
    d.borrow_mut().answer(
        ask.question(),
        crate::native_host::firmware_install::Answer::Yes,
    );
    loop {
        if matches!(
            d.borrow_mut().pump(&NoStorage, &Names, 100),
            Event::Ended(_)
        ) {
            break;
        }
    }
    let (status, output) = stage(
        &kernel,
        FIRMWARE,
        Some(&d),
        &[WireCommand::FirmwareRestart {}],
    );
    assert_eq!(status, OK);
    drain(&mut kernel, output);
    let ask = d.borrow().peek_ask().unwrap();
    assert!(ask.restart, "the host asks again");
}

fn install_plugin(request: u32, flags: u32) -> WireCommand<'static> {
    WireCommand::InstallPlugin {
        request,
        root: 0,
        path: "Download/dials",
        flags,
    }
}

fn author<'a>(request: u32, author: &'a [u8], recovery: &'a [u8]) -> WireCommand<'a> {
    author_with(request, author, recovery, &[])
}

fn author_with<'a>(
    request: u32,
    author: &'a [u8],
    recovery: &'a [u8],
    sha256: &'a [u8],
) -> WireCommand<'a> {
    WireCommand::InstallAuthor {
        request,
        author,
        recovery,
        sha256,
    }
}

fn rotation<'a>(request: u32, to: &'a [u8], signature: &'a [u8]) -> WireCommand<'a> {
    WireCommand::InstallRotation {
        request,
        to,
        signature,
    }
}

#[test]
fn an_install_needs_the_feature_a_plugin_host_known_flags_and_a_path_the_plugin_may_read() {
    let kernel = kernel();
    let d = plugin_driver();
    assert_eq!(
        stage(&kernel, FIRMWARE, Some(&d), &[install_plugin(1, 0)]).0,
        ERR_DENIED
    );
    // No plugin host: nothing to run it.
    let bare = driver();
    assert_eq!(
        stage(&kernel, INSTALL, Some(&bare), &[install_plugin(1, 0)]).0,
        ERR_UNSUPPORTED
    );
    assert_eq!(
        stage(&kernel, INSTALL, Some(&d), &[install_plugin(0, 0)]).0,
        ERR_INVALID
    );
    // Bits 0 (replace), 1 (older) and 2 (from a catalog) only.
    assert_eq!(
        stage(&kernel, INSTALL, Some(&d), &[install_plugin(1, 8)]).0,
        ERR_INVALID
    );
    let card = WireCommand::InstallPlugin {
        request: 1,
        root: 1,
        path: "Download/dials",
        flags: 0,
    };
    assert_eq!(stage(&kernel, INSTALL, Some(&d), &[card]).0, ERR_DENIED);
    assert_eq!(d.borrow().waiting(), 0, "a refusal accepts nothing");
    // One install a batch, like the other firmware commands.
    assert_eq!(
        stage(
            &kernel,
            INSTALL,
            Some(&d),
            &[install_plugin(1, 0), install_plugin(2, 0)]
        )
        .0,
        ERR_INVALID
    );
    // Accepted at submit; given back when the batch never applies.
    let (status, output) = stage(&kernel, INSTALL, Some(&d), &[install_plugin(1, 3)]);
    assert_eq!(status, OK);
    assert!(d.borrow().holds(OWNER, 1));
    drop(output);
    assert_eq!(d.borrow().waiting(), 0);
}

#[test]
fn evidence_belongs_to_the_install_before_it_and_is_none_or_whole() {
    let kernel = kernel();
    let d = plugin_driver();
    let (key, signature) = ([1_u8; 32], [2_u8; 64]);
    let run = |commands: &[WireCommand<'_>]| stage(&kernel, INSTALL, Some(&d), commands).0;
    // Before its install, or for another request: out of place.
    assert_eq!(
        run(&[author(1, &key, &[]), install_plugin(1, 0)]),
        ERR_INVALID
    );
    assert_eq!(
        run(&[install_plugin(1, 0), author(2, &key, &[])]),
        ERR_INVALID
    );
    assert_eq!(
        run(&[install_plugin(1, 0), rotation(2, &key, &signature)]),
        ERR_INVALID
    );
    // Evidence with no install at all.
    assert_eq!(run(&[author(1, &key, &[])]), ERR_INVALID);
    // At most one author.
    assert_eq!(
        run(&[
            install_plugin(1, 0),
            author(1, &key, &[]),
            author(1, &key, &[])
        ]),
        ERR_INVALID
    );
    // Keys are 0 or 32 bytes, a rotation's key 32 and its signature 64.
    for bad in [1_usize, 5, 31] {
        let short = [7_u8; 32];
        assert_eq!(
            run(&[install_plugin(1, 0), author(1, &short[..bad], &[])]),
            ERR_INVALID,
            "author of {bad} bytes"
        );
        assert_eq!(
            run(&[install_plugin(1, 0), author(1, &key, &short[..bad])]),
            ERR_INVALID,
            "recovery of {bad} bytes"
        );
    }
    assert_eq!(
        run(&[install_plugin(1, 0), rotation(1, &key[..31], &signature)]),
        ERR_INVALID
    );
    assert_eq!(
        run(&[install_plugin(1, 0), rotation(1, &key, &signature[..63])]),
        ERR_INVALID
    );
    // Six rotations, no more.
    let mut six = alloc::vec![install_plugin(1, 0), author(1, &key, &key)];
    for _ in 0..6 {
        six.push(rotation(1, &key, &signature));
    }
    let (status, output) = stage(&kernel, INSTALL, Some(&d), &six);
    assert_eq!(status, OK);
    drop(output);
    assert_eq!(d.borrow().waiting(), 0);
    // A seventh is refused by the host whatever the wire carries.
    let mut seen = EvidenceSeen::default();
    seen.install(1);
    for _ in 0..6 {
        assert_eq!(seen.take(&rotation(1, &key, &signature)), Ok(()));
    }
    assert_eq!(seen.take(&rotation(1, &key, &signature)), Err(ERR_INVALID));
}

#[test]
fn the_evidence_of_the_batch_reaches_the_job() {
    let kernel = kernel();
    let d = plugin_driver();
    let (key, recovery, signature) = ([1_u8; 32], [3_u8; 32], [2_u8; 64]);
    let (status, output) = stage(
        &kernel,
        INSTALL,
        Some(&d),
        &[
            install_plugin(5, 1),
            author_with(5, &key, &recovery, &[7; 32]),
            rotation(5, &[9; 32], &signature),
        ],
    );
    assert_eq!(status, OK);
    let (_, _, _, batch) = output;
    let [item] = batch.items.as_slice() else {
        panic!("one item")
    };
    let DevOp::Firmware(FirmwareOp::PluginInstall {
        request,
        flags,
        evidence,
        ..
    }) = &item.op
    else {
        panic!("an install")
    };
    assert_eq!((*request, *flags), (5, 1));
    let author = evidence.author.expect("the author record");
    assert_eq!(
        (author.author, author.recovery, author.sha256),
        (Some(key), Some(recovery), Some([7; 32])),
        "the catalog's digest reaches the job, which refuses other code with 18"
    );
    assert_eq!(evidence.rotations().len(), 1);
    assert_eq!(evidence.rotations()[0].to, [9; 32]);
    // Without the author record the install is a sideload.
    let (_, output) = stage(&kernel, INSTALL, Some(&d), &[install_plugin(6, 0)]);
    let (_, _, _, batch) = output;
    let DevOp::Firmware(FirmwareOp::PluginInstall { evidence, .. }) = &batch.items[0].op else {
        panic!("an install")
    };
    assert!(evidence.author.is_none() && evidence.rotations().is_empty());
}

#[test]
fn the_digest_is_none_or_32_bytes_and_a_wrong_length_is_refused_at_validation() {
    let kernel = kernel();
    let d = plugin_driver();
    let (key, sha) = ([1_u8; 32], [4_u8; 32]);
    let run = |commands: &[WireCommand<'_>]| stage(&kernel, INSTALL, Some(&d), commands).0;
    for bad in [1_usize, 16, 31] {
        assert_eq!(
            run(&[
                install_plugin(1, 0),
                author_with(1, &key, &key, &sha[..bad])
            ]),
            ERR_INVALID,
            "digest of {bad} bytes"
        );
    }
    assert_eq!(
        run(&[install_plugin(1, 0), author_with(1, &key, &key, &sha)]),
        OK
    );
    assert_eq!(
        run(&[install_plugin(1, 0), author_with(1, &key, &key, &[])]),
        OK
    );
}

#[test]
fn evidence_after_another_device_command_is_refused_at_validation() {
    let kernel = Kernel::builder(crate::tests::DEVICE)
        .wifi(true)
        .build()
        .unwrap();
    let d = plugin_driver();
    let key = [1_u8; 32];
    let run = |commands: &[WireCommand<'_>]| stage(&kernel, INSTALL | WIFI, Some(&d), commands).0;
    // Between the install and its evidence stands another command: the
    // batch is refused whole, before anything is taken.
    assert_eq!(
        run(&[
            install_plugin(1, 0),
            WireCommand::WifiOn {},
            author(1, &key, &[])
        ]),
        ERR_INVALID
    );
    assert_eq!(
        run(&[
            install_plugin(1, 0),
            WireCommand::RemovePlugin {
                request: 2,
                plugin: "x",
                flags: 0
            },
            author(1, &key, &[])
        ]),
        ERR_INVALID
    );
    assert_eq!(d.borrow().waiting(), 0, "nothing was accepted");
    // Another command before the install, or after its evidence, is fine.
    assert_eq!(
        run(&[
            WireCommand::WifiOn {},
            install_plugin(1, 0),
            author(1, &key, &[])
        ]),
        OK
    );
}

fn remove_plugin(request: u32, plugin: &str, flags: u32) -> WireCommand<'_> {
    WireCommand::RemovePlugin {
        request,
        plugin,
        flags,
    }
}

#[test]
fn a_removal_needs_the_feature_a_plugin_host_a_request_a_name_and_reserved_flags() {
    let kernel = kernel();
    let d = plugin_driver();
    assert_eq!(
        stage(&kernel, FIRMWARE, Some(&d), &[remove_plugin(1, "dials", 0)]).0,
        ERR_DENIED
    );
    let bare = driver();
    assert_eq!(
        stage(
            &kernel,
            INSTALL,
            Some(&bare),
            &[remove_plugin(1, "dials", 0)]
        )
        .0,
        ERR_UNSUPPORTED
    );
    for (request, plugin, flags) in [(0, "dials", 0), (1, "", 0), (1, "dials", 1)] {
        assert_eq!(
            stage(
                &kernel,
                INSTALL,
                Some(&d),
                &[remove_plugin(request, plugin, flags)]
            )
            .0,
            ERR_INVALID,
            "{request} {plugin:?} {flags}"
        );
    }
    // Evidence belongs to installs only.
    assert_eq!(
        stage(
            &kernel,
            INSTALL,
            Some(&d),
            &[remove_plugin(1, "dials", 0), author(1, &[1; 32], &[])]
        )
        .0,
        ERR_INVALID
    );
    // One firmware-queue command a batch.
    assert_eq!(
        stage(
            &kernel,
            INSTALL,
            Some(&d),
            &[remove_plugin(1, "dials", 0), remove_plugin(2, "dials", 0)]
        )
        .0,
        ERR_INVALID
    );
    // Accepted at submit; the name is checked by the job (a built-in or a
    // name no plugin has ends with reason 15), and given back with the batch.
    let (status, output) = stage(&kernel, INSTALL, Some(&d), &[remove_plugin(1, "dials", 0)]);
    assert_eq!(status, OK);
    assert!(d.borrow().holds(OWNER, 1));
    drop(output);
    assert_eq!(d.borrow().waiting(), 0);
}

fn revoked_cmd<'a>(plugin: &'a str, key: &'a [u8], signature: &'a [u8]) -> WireCommand<'a> {
    WireCommand::TrustRevoked {
        plugin,
        key,
        signature,
    }
}

#[test]
fn trust_revoked_is_checked_whole_and_capped_at_eight_per_batch() {
    let kernel = kernel();
    let d = driver();
    let (key, signature) = ([1_u8; 32], [2_u8; 64]);
    let run = |commands: &[WireCommand<'_>]| stage(&kernel, INSTALL, Some(&d), commands).0;
    let one = revoked_cmd("notes", &key, &signature);
    assert_eq!(run(&[one]), OK);
    // The batch holds eight records at most, so the cap of eight is also what
    // the wire carries; the host's own count is the second line.
    assert_eq!(run(&[one; 8]), OK, "the cap itself");
    for (what, bad) in [
        ("empty plugin", revoked_cmd("", &key, &signature)),
        ("key of 31", revoked_cmd("notes", &key[..31], &signature)),
        ("key of 0", revoked_cmd("notes", &[], &signature)),
        ("signature 63", revoked_cmd("notes", &key, &signature[..63])),
        ("signature 0", revoked_cmd("notes", &key, &[])),
    ] {
        assert_eq!(run(&[one, bad]), ERR_INVALID, "{what}");
    }
    // The feature and a firmware host are needed, as for any install command.
    assert_eq!(
        stage(&kernel, FIRMWARE, Some(&d), &[one]).0,
        ERR_DENIED,
        "no uses.install"
    );
    assert_eq!(stage(&kernel, INSTALL, None, &[one]).0, ERR_UNSUPPORTED);
    // It is no firmware command: one install_firmware still fits beside it.
    assert_eq!(
        stage(
            &kernel,
            INSTALL | FIRMWARE,
            Some(&d),
            &[install(1, "u.ppu"), one]
        )
        .0,
        OK
    );
    assert_eq!(
        d.borrow().waiting(),
        0,
        "none of it was accepted before a drain"
    );
}

#[test]
fn a_revocation_that_holds_is_kept_at_the_next_pump_and_the_others_are_dropped() {
    use paperpaw_native::signature::{public_key, sign_message};
    use paperpaw_sdk::{MemoryStorage, native::keys};
    let card = MemoryStorage::new();
    let (author_seed, recovery_seed) = ([3_u8; 32], [4_u8; 32]);
    let (author, recovery) = (public_key(&author_seed), public_key(&recovery_seed));
    assert!(keys::remember_first_install(
        &card, "notes", &author, &recovery
    ));
    let say = |plugin: &str, seed: &[u8; 32]| {
        sign_message(keys::revocation_message(plugin, &author).as_bytes(), seed)
    };
    let (holds, forged) = (say("notes", &recovery_seed), say("notes", &[9; 32]));
    let mut kernel = kernel();
    let d = plugin_driver();
    let sent = |kernel: &Kernel, signature: &[u8; 64]| {
        stage(
            kernel,
            INSTALL,
            Some(&d),
            &[revoked_cmd("notes", &author, signature)],
        )
    };
    let (status, output) = sent(&kernel, &forged);
    assert_eq!(status, OK);
    drain(&mut kernel, output);
    assert!(d.borrow().has_work(0), "it waits for the pump");
    d.borrow_mut().pump(&card, &Names, 100);
    assert!(
        !keys::is_revoked(&card, "notes"),
        "a forged one holds nothing"
    );
    assert!(!d.borrow().has_work(0));
    let (status, output) = sent(&kernel, &holds);
    assert_eq!(status, OK);
    drain(&mut kernel, output);
    d.borrow_mut().pump(&card, &Names, 100);
    assert!(keys::is_revoked(&card, "notes"));
    // Taken once, never off: the same again changes nothing.
    let (_, output) = sent(&kernel, &holds);
    drain(&mut kernel, output);
    d.borrow_mut().pump(&card, &Names, 100);
    assert_eq!(keys::revoked(&card).len(), 1);
}

#[test]
fn revocations_past_the_queue_are_dropped_and_counted() {
    use paperpaw_sdk::native::keys::Revocation;
    let d = driver();
    for _ in 0..crate::native_host::firmware_driver::MAX_REVOCATIONS_QUEUED {
        assert!(
            d.borrow_mut()
                .queue_revocation(Revocation::new("notes", [1; 32], [2; 64]))
        );
    }
    assert!(
        !d.borrow_mut()
            .queue_revocation(Revocation::new("notes", [1; 32], [2; 64]))
    );
}

fn dual_boot(request: u32, mode: u32) -> WireCommand<'static> {
    WireCommand::FirmwareDualBoot { request, mode }
}

#[test]
fn firmware_dual_boot_is_unsupported_until_the_host_opens_it() {
    use crate::native_host::plugin_job::PluginHost;
    use paperpaw_native::{
        header5::{Host as Loader, X4_PROFILE},
        metadata::Host as Offered,
    };
    let kernel = kernel();
    let d = driver();
    d.borrow_mut().set_plugin_host(PluginHost {
        loader: Loader::x4(1),
        offered: Offered {
            abi_major: 7,
            minor: 1,
            profile: X4_PROFILE,
            services: &[],
            features: &[],
        },
        signers: &[],
    });
    // The feature first, then the gate: nothing is queued either way.
    assert_eq!(
        stage(&kernel, 0, Some(&d), &[dual_boot(1, 1)]).0,
        ERR_DENIED
    );
    assert_eq!(
        stage(&kernel, FIRMWARE, None, &[dual_boot(1, 1)]).0,
        ERR_UNSUPPORTED
    );
    assert_eq!(
        stage(&kernel, FIRMWARE, Some(&d), &[dual_boot(1, 1)]).0,
        ERR_UNSUPPORTED
    );
    assert_eq!(d.borrow().waiting(), 0);
    d.borrow_mut().open_dual_boot();
    // A mode beyond 1 and request 0 are invalid at submit, nothing queued.
    for command in [dual_boot(1, 2), dual_boot(0, 1)] {
        assert_eq!(
            stage(&kernel, FIRMWARE, Some(&d), &[command]).0,
            ERR_INVALID
        );
    }
    assert_eq!(d.borrow().waiting(), 0);
    // Both modes are accepted into the queue, one firmware command a batch.
    let (status, output) = stage(&kernel, FIRMWARE, Some(&d), &[dual_boot(1, 0)]);
    assert_eq!(status, OK);
    drop(output);
    assert_eq!(
        stage(
            &kernel,
            FIRMWARE,
            Some(&d),
            &[dual_boot(2, 1), dual_boot(3, 1)]
        )
        .0,
        ERR_INVALID
    );
}
