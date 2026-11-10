//! The network policy of the native services in a kernel with `core.net`
//! wired: closed until the kernel has filled it, and as current as the user's
//! last decision for the very next callback of a plugin.
use super::*;
use crate::native_host::{
    net_policy::NetPolicy,
    net_service::reason,
    shell_wiring::{
        HostWiring, HttpJob, NetInputs, NetLimits, ServedReply, ServerReason, ShellHttp,
        ShellNetwork,
    },
};
use paperpaw_platform::Storage;
use paperpaw_sdk::{MemoryStorage, SettingValue};

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

type Seen = Rc<RefCell<Option<Rc<NetPolicy>>>>;

/// A plugin that uses the network and, whenever a setting changes, notes what
/// the services would answer it at that moment (what its own next request in
/// that callback would meet).
struct Asker {
    id: &'static str,
    policy: Seen,
    log: Log,
}

impl Plugin for Asker {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static(self.id), "Asker", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let (policy, log) = (self.policy.clone(), self.log.clone());
        // The very first thing a plugin hears: what it would meet then.
        let id = self.id;
        // What the network manager hears of the demand.
        for (event, what) in [
            (
                paperpaw_sdk::builtin::events::NETWORK_WANTED.id().clone(),
                "wanted",
            ),
            (
                paperpaw_sdk::builtin::events::NETWORK_IDLE.id().clone(),
                "idle",
            ),
        ] {
            let log = log.clone();
            r.subscribe(
                event,
                Box::new(move |_, _, _| log.borrow_mut().push(alloc::format!("{id} {what}"))),
            )?;
        }
        let (first_policy, first_log) = (policy.clone(), log.clone());
        r.subscribe(
            paperpaw_sdk::builtin::events::STARTED.id().clone(),
            Box::new(move |_, _, _| {
                let end = first_policy
                    .borrow()
                    .as_ref()
                    .and_then(|p| p.end_for(id, false))
                    .map(|e| e.reason);
                first_log
                    .borrow_mut()
                    .push(alloc::format!("{id} started {end:?}"));
            }),
        )?;
        if id != "wk" {
            return Ok(());
        }
        r.subscribe(
            paperpaw_sdk::builtin::events::SETTING_CHANGED.id().clone(),
            Box::new(move |_, params, _| {
                if params.get(paperpaw_sdk::builtin::params::ID) != Some("core.network.wk") {
                    return;
                }
                let end = policy
                    .borrow()
                    .as_ref()
                    .and_then(|p| p.end_for("wk", false))
                    .map(|e| e.reason);
                log.borrow_mut().push(alloc::format!("{end:?}"));
            }),
        )?;
        Ok(())
    }
}

/// A kernel with the plugins `wk` and `ok` over `storage`, `core.net` wired
/// but the kernel not yet started: what a shell has done at boot.
fn booting(storage: impl paperpaw_platform::Storage + 'static) -> (Kernel, Rc<NetPolicy>, Log) {
    booting_with(storage, false)
}

/// As [`booting`], with Wi-Fi managed by PaperPaw if `wifi`.
pub(super) fn booting_with(
    storage: impl paperpaw_platform::Storage + 'static,
    wifi: bool,
) -> (Kernel, Rc<NetPolicy>, Log) {
    let (log, seen) = (log(), Seen::default());
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(Asker {
            id: "wk",
            policy: seen.clone(),
            log: log.clone(),
        })
        .plugin(Asker {
            id: "ok",
            policy: seen.clone(),
            log: log.clone(),
        })
        .storage(storage)
        .network(true)
        .wifi(wifi)
        .build()
        .unwrap();
    // A plugin instance brings the host resources of the device; the shell
    // wires `core.net` into them.
    kernel
        .shared
        .native_instances
        .test_insert(Id::new_static("wk"), None, true)
        .unwrap();
    let shell = Rc::new(Nothing);
    let net = NetInputs::new(
        Rc::new(MemoryStorage::new()),
        shell.clone(),
        shell,
        NetLimits::new(49_152, 2),
    );
    let resources = kernel.shared.native_instances.test_resources().unwrap();
    resources.wire(HostWiring::new().net(net)).unwrap();
    let policy = resources.hosts().policy().clone();
    *seen.borrow_mut() = Some(policy.clone());
    (kernel, policy, log)
}

fn wired() -> (Kernel, Rc<NetPolicy>, Log) {
    let (mut kernel, policy, log) = booting(MemoryStorage::new());
    kernel.start(Instant(0));
    log.borrow_mut().clear();
    (kernel, policy, log)
}

fn network_of_wk(on: bool) -> Command {
    Command::SetSetting(Id::new_static("core.network.wk"), SettingValue::Bool(on))
}

#[test]
fn a_policy_the_kernel_has_not_filled_answers_every_plugin_as_off() {
    // What a callback before the first call of the kernel would meet.
    let (_kernel, policy, _) = booting(MemoryStorage::new());
    assert_eq!(
        policy.end_for("wk", false).map(|e| e.reason),
        Some(reason::NETWORK_OFF)
    );
}

#[test]
fn the_first_pass_fills_it_and_a_plugin_that_is_allowed_is_let_through() {
    let (mut kernel, policy, _) = wired();
    kernel.tick(Instant(10));
    assert_eq!(policy.end_for("wk", false), None);
    assert_eq!(policy.end_for("wk", true), None);
}

#[test]
fn a_setting_turned_off_is_known_to_the_next_callback_even_in_the_same_call() {
    let (mut kernel, policy, log) = wired();
    // Turned off before anything filled the policy: it is never open.
    kernel.now = Instant(5);
    kernel.apply(network_of_wk(false));
    kernel.run_commands();
    assert_eq!(
        policy.end_for("wk", false).map(|e| e.reason),
        Some(reason::NETWORK_OFF)
    );
    assert_eq!(*log.borrow(), ["Some(13)"], "the plugin's own callback");
    // The call ends and the next one starts: still off.
    kernel.finish();
    kernel.tick(Instant(20));
    assert_eq!(
        policy.end_for("wk", false).map(|e| e.reason),
        Some(reason::NETWORK_OFF)
    );
    // Turned on again, it is let through as soon as the setting is applied.
    kernel.now = Instant(30);
    kernel.apply(network_of_wk(true));
    kernel.run_commands();
    assert_eq!(policy.end_for("wk", false), None);
    assert_eq!(log.borrow().last().unwrap(), "None");
}

#[test]
fn at_boot_a_plugin_disabled_in_the_saved_settings_is_refused_and_an_allowed_one_is_let_through() {
    let storage = MemoryStorage::new();
    storage
        .write(
            paperpaw_sdk::builtin::CONFIG_FILE,
            b"[core]\nnetwork.wk = off\n",
        )
        .unwrap();
    let (mut kernel, policy, log) = booting(storage);
    // `start` ends in `finish`, which fills the policy before it delivers
    // `STARTED` (the plugins' first callback).
    kernel.start(Instant(0));
    // The first thing each plugin heard was answered by the user's choice,
    // not by an unfilled policy: no allowed plugin was told `network_off`.
    let log = log.borrow();
    assert!(
        log.contains(&"wk started Some(13)".into()),
        "the disabled one is refused: {log:?}"
    );
    assert!(
        log.contains(&"ok started None".into()),
        "the allowed one is let through: {log:?}"
    );
    assert_eq!(policy.end_for("ok", true), None);
    assert_eq!(
        policy.end_for("wk", true).map(|e| e.reason),
        Some(reason::NETWORK_OFF)
    );
}
