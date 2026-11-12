//! `core.device` ops 3, 6 and 16 over the firmware install machine: the
//! numbering of the built-ins is op 4's, a check is read by its request, and a
//! foreign image has no list.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::*;
use crate::native_host::firmware_install::{Answer, Event, FirmwareInstall, Job, Question};
use core::cell::RefCell;
use paperpaw_platform::{
    NoStorage,
    firmware::{Signer, WRITE_BLOCK},
    firmware_fake::{FakeFirmware, paperpaw_facts},
};

const OWNER: u64 = 5;

fn host(ids: &[&str]) -> Fake {
    Fake {
        builtin: Some(
            ids.iter()
                .map(|id| ((*id).to_string(), "1.0.0".to_string()))
                .collect(),
        ),
        ..Fake::full()
    }
}

/// The image has built in exactly `has`.
fn image(has: &[&str]) -> FakeFirmware {
    let mut fake = FakeFirmware::new(
        "0.5.0",
        "paperpaw-x4",
        5,
        paperpaw_facts("0.6.0", "paperpaw-x4", 3 * WRITE_BLOCK),
    );
    fake.built_ins = has.iter().map(|id| (*id).to_string()).collect();
    fake
}

fn drive(machine: &mut FirmwareInstall, fw: &mut FakeFirmware, host: &Fake) -> Event {
    let host = Fake {
        builtin: host.builtin.clone(),
        ..Fake::default()
    };
    let rows = DeviceBuiltIns(&host);
    for now in (100..10_000).step_by(100) {
        match machine.step(fw, &NoStorage, &rows, now) {
            Event::Quiet | Event::Progress { .. } => {}
            other => return other,
        }
    }
    panic!("never ended");
}

/// A machine that finished `OWNER`'s check (request 7) of `fw`, wired.
fn checked<R>(
    ids: &[&str],
    mut fw: FakeFirmware,
    caller: u64,
    then: impl FnOnce(&View<'_>, &RefCell<FirmwareInstall>) -> R,
) -> R {
    let machine =
        crate::native_host::firmware_cell::test_shared(RefCell::new(FirmwareInstall::new()));
    {
        let mut machine = machine.try_borrow_mut().unwrap();
        machine.reserve_check(OWNER).unwrap();
        machine.start(Job::check(OWNER, 7, "update.ppu").unwrap(), &mut fw);
        assert!(matches!(
            drive(&mut machine, &mut fw, &host(ids)),
            Event::Ended(_)
        ));
    }
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let hosts = crate::native_host::host_services::HostServices::new();
    hosts.wire_device(Rc::new(host(ids))).unwrap();
    hosts
        .wire_firmware_install(machine.try_clone().unwrap())
        .unwrap();
    then(
        &View {
            env: &env,
            hosts,
            owner: caller,
        },
        &machine,
    )
}

fn request(number: u32) -> Vec<u8> {
    batch(&[Record::ServiceDevRequest(
        registrations::ServiceDevRequest { request: number },
    )])
}

#[test]
fn op_16_lists_the_built_ins_the_image_lacks_by_op_4s_numbers_empty_ids_skipped() {
    // Op 4 lists a, b, c, d (the empty id is no row); the image has c and a.
    let ids = ["", "a", "b", "c", "d"];
    checked(&ids, image(&["c", "a"]), OWNER, |view, _| {
        let four = run(view, 4, &query(0, 0)).unwrap();
        let listed: Vec<&str> = records(&four)
            .iter()
            .filter_map(|record| match record {
                Record::ServiceDevBuiltin(row) => Some(row.id),
                _ => None,
            })
            .collect();
        assert_eq!(listed, ["a", "b", "c", "d"]);
        let reply = run(view, 16, &query(0, 0)).unwrap();
        let rows = records(&reply);
        let left: Vec<&str> = rows
            .iter()
            .filter_map(|record| match record {
                Record::ServiceDevBuiltin(row) => Some(row.id),
                _ => None,
            })
            .collect();
        assert_eq!(left, ["b", "d"], "the rows op 4 numbers 1 and 3");
        let end = end_of(&rows);
        assert_eq!((end.total, end.generation, end.flags), (2, 7, 1));
        // The `end`'s generation is the check's request: it pages on.
        assert!(run(view, 16, &query(1, 7)).is_ok());
        // A cursor of another check is ERR_STATE; so is a cursor without one.
        assert_eq!(run(view, 16, &query(1, 6)).unwrap_err(), ERR_STATE);
        assert_eq!(run(view, 16, &query(1, 0)).unwrap_err(), ERR_STATE);
    });
}

#[test]
fn op_16_refuses_what_is_not_a_list_and_leaves_none_out_where_none_is() {
    checked(&["a", "b"], image(&["a", "b"]), OWNER, |view, _| {
        let reply = run(view, 16, &query(0, 0)).unwrap();
        let rows = records(&reply);
        assert_eq!(rows.len(), 1, "only the end");
        let scoped = batch(&[Record::ServiceRegQuery(registrations::ServiceRegQuery {
            scope: "x",
            cursor: 0,
            generation: 0,
            flags: 0,
        })]);
        assert_eq!(run(view, 16, &scoped).unwrap_err(), ERR_INVALID);
        let reply = run(view, 6, &request(7)).unwrap();
        let Record::ServiceDevCheck(check) = records(&reply)[0] else {
            panic!("not a check");
        };
        assert_eq!(check.flags & (1 << 5), 0, "none left out: bit 5 clear");
    });
    // Another plugin has no check; a foreign image has no list.
    checked(&["a"], image(&[]), 9, |view, _| {
        assert_eq!(run(view, 16, &query(0, 0)).unwrap_err(), ERR_NOT_FOUND);
        assert_eq!(run(view, 6, &request(7)).unwrap_err(), ERR_NOT_FOUND);
    });
    let mut foreign = image(&["a"]);
    foreign.facts.foreign = true;
    foreign.facts.signer = Signer::Unsigned;
    checked(&["a"], foreign, OWNER, |view, _| {
        assert_eq!(run(view, 16, &query(0, 0)).unwrap_err(), ERR_UNSUPPORTED);
    });
}

#[test]
fn op_6_reads_the_check_by_its_request_with_the_key_the_flags_and_the_digest() {
    checked(&["a", "b"], image(&["a"]), OWNER, |view, _| {
        let reply = run(view, 6, &request(7)).unwrap();
        let Record::ServiceDevCheck(check) = records(&reply)[0] else {
            panic!("not a check");
        };
        assert_eq!(check.reason, 0);
        assert_eq!(check.bytes, u64::from(3 * WRITE_BLOCK));
        assert_eq!((check.id, check.version), ("paperpaw-x4", "0.6.0"));
        assert_eq!(
            (check.signer, check.key, check.sha256),
            ("PaperPaw", &[7; 32][..], &[9; 32][..])
        );
        // Signed by PaperPaw (bit 3) and one built-in left out (bit 5).
        assert_eq!(check.flags, 1 << 3 | 1 << 5);
        assert_eq!(run(view, 6, &request(8)).unwrap_err(), ERR_NOT_FOUND);
    });
    // An unknown key's check tells the key (what the user's own builds need).
    let mut other = image(&["a"]);
    other.facts.signer = Signer::Other { key: [3; 32] };
    checked(&["a"], other, OWNER, |view, _| {
        let reply = run(view, 6, &request(7)).unwrap();
        let Record::ServiceDevCheck(check) = records(&reply)[0] else {
            panic!("not a check");
        };
        assert_eq!(
            (check.reason, check.key, check.signer),
            (9, &[3; 32][..], "")
        );
    });
}

#[test]
fn op_3_reads_the_install_state_from_the_machine_and_the_rest_from_the_host() {
    let ids = ["a"];
    checked(&ids, image(&["a"]), OWNER, |view, machine| {
        let state = |view: &View<'_>| {
            let reply = run(view, 3, &[0]).unwrap();
            let Record::ServiceDevFirmware(f) = records(&reply)[0] else {
                panic!("not firmware");
            };
            (f.state, f.request, f.reason, f.version.to_string(), f.done)
        };
        // Idle: the host's version, the machine's state (the host said 9).
        assert_eq!(state(view), (0, 0, 0, "1.2.3".to_string(), 0));
        // An install waiting for the user's answer.
        let mut fw = image(&["a"]);
        let host = host(&ids);
        let mut machine = machine.try_borrow_mut().unwrap();
        machine.start(Job::install(OWNER, 8, "u.ppu", None, 0).unwrap(), &mut fw);
        assert!(matches!(drive(&mut machine, &mut fw, &host), Event::Ask(_)));
        drop(machine);
        assert_eq!(state(view).0, 1);
    });
}

#[test]
fn op_3_with_no_machine_is_the_hosts_facts_unchanged() {
    let reply = call(Fake::full(), 3, &[0]).unwrap();
    let Record::ServiceDevFirmware(f) = records(&reply)[0] else {
        panic!("not firmware");
    };
    assert_eq!((f.state, f.request, f.done, f.total), (4, 4, 10, 20));
    let _ = (
        Answer::Yes,
        Question {
            owner: 0,
            request: 0,
            restart: false,
        },
    );
}

#[test]
fn op_3_tells_the_caller_only_its_own_request_and_op_16_pages_past_a_zero_request() {
    let ids = ["a"];
    // Another plugin's install waits for the user: this caller sees no request.
    checked(&ids, image(&["a"]), 9, |view, machine| {
        let mut fw = image(&["a"]);
        let host = host(&ids);
        let mut machine = machine.try_borrow_mut().unwrap();
        machine.start(Job::install(OWNER, 8, "u.ppu", None, 0).unwrap(), &mut fw);
        assert!(matches!(drive(&mut machine, &mut fw, &host), Event::Ask(_)));
        drop(machine);
        let reply = run(view, 3, &[0]).unwrap();
        let Record::ServiceDevFirmware(f) = records(&reply)[0] else {
            panic!("not firmware");
        };
        assert_eq!((f.state, f.request, f.reason), (1, 0, 0));
    });
    // 40 built-ins, all left out, a check with request 0: the second page
    // follows the `end`'s generation (never 0) and ends the list.
    let names: Vec<String> = (0..40).map(|n| alloc::format!("p{n}")).collect();
    let ids: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut machine = FirmwareInstall::new();
    let mut fw = image(&[]);
    machine.reserve_check(OWNER).unwrap();
    machine.start(Job::check(OWNER, 0, "update.ppu").unwrap(), &mut fw);
    assert!(matches!(
        drive(&mut machine, &mut fw, &host(&ids)),
        Event::Ended(_)
    ));
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let hosts = crate::native_host::host_services::HostServices::new();
    hosts.wire_device(Rc::new(host(&ids))).unwrap();
    hosts
        .wire_firmware_install(crate::native_host::firmware_cell::test_shared(
            RefCell::new(machine),
        ))
        .unwrap();
    let view = View {
        env: &env,
        hosts,
        owner: OWNER,
    };
    let first = run(&view, 16, &query(0, 0)).unwrap();
    let rows = records(&first);
    let end = end_of(&rows);
    assert_eq!((rows.len(), end.total, end.flags), (33, 40, 0));
    assert_ne!(end.generation, 0);
    let second = run(&view, 16, &query(end.next, end.generation)).unwrap();
    let rows = records(&second);
    assert_eq!(
        (rows.len(), end_of(&rows).flags),
        (9, 1),
        "8 rows and the end"
    );
}

#[test]
fn op_13_tells_a_request_waiting_running_and_ended_and_only_the_askers() {
    let ids = ["a"];
    let fake_host = host(&ids);
    let machine =
        crate::native_host::firmware_cell::test_shared(RefCell::new(FirmwareInstall::new()));
    let driver = crate::native_host::firmware_cell::test_shared(RefCell::new(
        crate::native_host::firmware_driver::Driver::new(
            alloc::boxed::Box::new(image(&["a"])),
            machine.try_clone().unwrap(),
        ),
    ));
    driver
        .borrow_mut()
        .submit_check(OWNER, 7, "update.ppu")
        .unwrap();
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let view = |owner| {
        let hosts = crate::native_host::host_services::HostServices::new();
        hosts.wire_device(Rc::new(host(&ids))).unwrap();
        hosts
            .wire_firmware_install(machine.try_clone().unwrap())
            .unwrap();
        hosts
            .wire_firmware_requests(driver.try_clone().unwrap())
            .unwrap();
        View {
            env: &env,
            hosts,
            owner,
        }
    };
    let state = |reply: &[u8]| {
        let Record::ServiceDevInstall(install) = records(reply)[0] else {
            panic!("not an install");
        };
        (install.kind, install.state, install.status, install.reason)
    };
    // Waiting in the queue; another plugin and another number know nothing.
    assert_eq!(
        state(&run(&view(OWNER), 13, &request(7)).unwrap()),
        (3, 0, 0, 0)
    );
    assert_eq!(
        run(&view(OWNER), 13, &request(8)).unwrap_err(),
        ERR_NOT_FOUND
    );
    assert_eq!(run(&view(9), 13, &request(7)).unwrap_err(), ERR_NOT_FOUND);
    // It runs, then ends: kept once its edge was delivered.
    let rows = DeviceBuiltIns(&fake_host);
    let ended = loop {
        if let Event::Ended(outcome) = driver.borrow_mut().pump(&NoStorage, &rows, 100) {
            break outcome;
        }
    };
    // Ended: told as such before and after its edge is delivered.
    assert_eq!(
        state(&run(&view(OWNER), 13, &request(7)).unwrap()),
        (3, 2, 0, 0)
    );
    let _ = driver.borrow_mut().take_ready();
    driver.borrow_mut().delivered(ended);
    assert_eq!(
        state(&run(&view(OWNER), 13, &request(7)).unwrap()),
        (3, 2, 0, 0)
    );
}
