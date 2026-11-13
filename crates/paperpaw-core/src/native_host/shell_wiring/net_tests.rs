#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The wiring of `core.net`: once per device, nothing half wired, and the
//! reports reach the service.
extern crate std;
use super::*;
use crate::native_host::resources::HostResources;
use paperpaw_native::host_glue::{ERR_NOT_FOUND, ERR_STATE};
use paperpaw_sdk::MemoryStorage;

struct Nothing;
impl ShellHttp for Nothing {
    fn start(&self, _: &HttpJob<'_>) -> Result<(), i32> {
        Ok(())
    }
    fn abort(&self, _: u32) {}
    fn listen(&self, _: u32, _: u32, _: &mut [u8]) -> Result<usize, ServerReason> {
        Err(ServerReason::Refused)
    }
    fn unlisten(&self, _: u32) {}
    fn reply(&self, _: u32, _: &ServedReply<'_>) -> Result<(), i32> {
        Ok(())
    }
    fn close_served(&self, _: u32, _: u32) {}
}
impl ShellNetwork for Nothing {
    fn now_ms(&self) -> u64 {
        0
    }
    fn network_up(&self) -> bool {
        true
    }
    fn network_demand(&self, _: bool) {}
}

fn inputs() -> NetInputs {
    let shell = Rc::new(Nothing);
    NetInputs::new(
        Rc::new(MemoryStorage::new()),
        shell.clone(),
        shell,
        NetLimits::new(49_152, 2),
    )
}

#[test]
fn a_shell_wires_the_network_once() {
    let resources = HostResources::try_new().unwrap();
    let wired = resources.wire(HostWiring::new().net(inputs())).unwrap();
    assert!(wired.net().is_some() && resources.hosts().net_wired());
    // Again: refused whole, and nothing else named is wired by that call.
    let again = resources.wire(
        HostWiring::new()
            .net(inputs())
            .hyphenation(Rc::new(MemoryStorage::new())),
    );
    assert_eq!(again.err(), Some(ERR_STATE));
    assert!(resources.hosts().hyphen().is_none());
}

#[test]
fn reports_reach_the_service_and_unknown_ids_are_ignored() {
    let resources = HostResources::try_new().unwrap();
    let wired = resources.wire(HostWiring::new().net(inputs())).unwrap();
    let reports = wired.net().unwrap();
    assert_eq!(
        reports.finished(99, HttpDone::new(200, HttpReason::Ok)),
        Ok(())
    );
    assert_eq!(reports.progress(99, 200, 1, None), Ok(()));
    assert_eq!(reports.tick(), Ok(()));
    assert_eq!(reports.network_lost(), Ok(()));
    assert_eq!(reports.served_gone(5), Ok(()));
    assert_eq!(reports.served_whole(5, 0), Err(ERR_NOT_FOUND));
    assert_eq!(
        reports.served_head(1, "GET", "/", None, false, 10),
        Err(ERR_NOT_FOUND)
    );
}

#[test]
fn every_shell_reason_has_its_wire_value() {
    // `network.md` 4.1: 0 ok, 1 no_network, 2 dns, 3 tls, 4 timeout, 5 refused,
    // 6 too_large, 9 io, 10 full.
    for (reason, wire) in [
        (HttpReason::Ok, 0),
        (HttpReason::NoNetwork, 1),
        (HttpReason::Dns, 2),
        (HttpReason::Tls, 3),
        (HttpReason::Timeout, 4),
        (HttpReason::Refused, 5),
        (HttpReason::TooLarge, 6),
        (HttpReason::Io, 9),
        (HttpReason::Full, 10),
    ] {
        let finished = HttpDone::new(0, reason).into_finished();
        assert_eq!(finished.reason, wire, "{reason:?}");
    }
}

#[test]
fn a_call_naming_the_network_and_core_files_together_wires_both() {
    use crate::native_host::storage_files::{BusyKind, BusySource};
    struct Quiet;
    impl BusySource for Quiet {
        fn busy(
            &self,
            _: &dyn paperpaw_platform::Storage,
            _: &str,
            _: crate::native_host::files_service::Access,
        ) -> bool {
            false
        }
    }
    let resources = HostResources::try_new().unwrap();
    let sources = resources.hosts().busy_sources();
    sources
        .register(BusyKind::LibraryMoves, Rc::new(Quiet))
        .unwrap();
    resources.hosts().register_reader_book_reporting().unwrap();
    let wired = resources
        .wire(
            HostWiring::new()
                .net(inputs())
                .files(Rc::new(MemoryStorage::new())),
        )
        .unwrap();
    assert!(wired.net().is_some());
    assert!(resources.hosts().net_wired() && resources.hosts().files().is_some());
}
