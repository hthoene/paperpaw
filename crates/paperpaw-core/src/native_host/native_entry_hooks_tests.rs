//! The shell is told of every entry of a card plugin's code
//! (`Services::enter` and `leave`): `make`, callbacks and the destructor, and
//! may refuse any of them (a refused destructor leaks).
use super::native_screen_install::{DESTROYED, install, kernel, open};
use super::*;
use paperpaw_sdk::{Id, Params};

std::thread_local! {
    /// The hosts made by `native_screen_install` are announced to the shell,
    /// as `HostedAdmission::new` does for a plugin admitted from a file.
    static ANNOUNCED: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn announce_if_set(hosted: &Hosted<Heap, ()>) {
    if ANNOUNCED.with(Cell::get) {
        hosted.owner().context().announce_entries();
    }
}

fn announced() {
    ANNOUNCED.with(|value| value.set(true));
}

fn counts() -> (u32, u32) {
    (ENTERED.with(Cell::get), LEFT.with(Cell::get))
}

#[test]
fn make_a_callback_and_the_destructor_are_announced_and_each_one_ends() {
    announced();
    let mut kernel = kernel(0);
    let (entered, left) = counts();
    assert!(entered >= 1, "make was not announced");
    assert_eq!(entered, left, "an entry did not end");
    open(&mut kernel, Params::new());
    let (after_open, left_after_open) = counts();
    assert!(after_open > entered, "a callback was not announced");
    assert_eq!(after_open, left_after_open);
    kernel.unload_plugin(&Id::new_static("factory"));
    let (after_unload, left_after_unload) = counts();
    assert!(
        after_unload > after_open,
        "the destructor was not announced"
    );
    assert_eq!(after_unload, left_after_unload);
    assert_eq!(DESTROYED.with(Cell::get), 1);
}

#[test]
fn a_refused_make_admits_nothing_and_runs_no_plugin_code() {
    announced();
    reset(NORMAL);
    DESTROYED.with(|value| value.set(0));
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    REFUSE_ENTRY.with(|refuse| refuse.set(true));
    let result = install(&mut kernel, 0);
    assert!(matches!(result, Err(Error::Refused)), "{result:?}");
    // Announced once, refused: no `leave`, no plugin, nothing registered.
    assert_eq!(counts(), (1, 0));
    assert!(kernel.shared.native_instances.find("factory").is_none());
    assert!(
        !kernel
            .shared
            .registry
            .screens
            .contains_key("factory.screen")
    );
    assert_eq!(DESTROYED.with(Cell::get), 0);
}

#[test]
fn a_refused_callback_does_not_run_and_neither_does_the_destructor() {
    announced();
    let mut kernel = kernel(0);
    let (entered, left) = counts();
    REFUSE_ENTRY.with(|refuse| refuse.set(true));
    open(&mut kernel, Params::new());
    let (now_entered, now_left) = counts();
    // Announced and refused: the plugin's code did not run, so nothing left.
    assert!(now_entered > entered);
    assert_eq!(now_left, left, "a refused entry must not be ended");
    // The destructor is refused like any entry: it does not run (the shell
    // keeps plugin code from running without its guard), the blocks leak.
    kernel.unload_plugin(&Id::new_static("factory"));
    assert_eq!(DESTROYED.with(Cell::get), 0);
    assert_eq!(counts().1, now_left, "a refused destructor is not ended");
    REFUSE_ENTRY.with(|refuse| refuse.set(false));
}

#[test]
fn the_shell_is_given_the_plugins_id_before_its_make_and_every_entry_ends() {
    // The X4 names that id to safe mode (`safe_mode::calling`) for as long as
    // plugin code runs: a crash in `make` leaves it set and the next start
    // switches that plugin off (the id must be the plugin's, not another name).
    announced();
    let mut kernel = kernel(0);
    open(&mut kernel, Params::new());
    kernel.unload_plugin(&Id::new_static("factory"));
    let ids = ENTER_IDS.with(|ids| ids.borrow().clone());
    assert!(ids.len() >= 3, "{ids:?}");
    assert!(ids.iter().all(|id| id == "factory"), "{ids:?}");
    assert_eq!(counts().0, counts().1, "every entry ended");
}

/// A shell whose `load_native` hands the kernel a hosted plugin to admit.
struct HostedLoader;

impl crate::CodeLoader for HostedLoader {
    fn load(
        &mut self,
        _: &dyn paperpaw_sdk::Storage,
        _: &crate::CodeFile<'_>,
    ) -> Result<Box<dyn paperpaw_sdk::Plugin>, paperpaw_sdk::PluginProblem> {
        Err(paperpaw_sdk::PluginProblem::Damaged)
    }

    fn load_native(
        &mut self,
        _: &dyn paperpaw_sdk::Storage,
        _: &crate::CodeFile<'_>,
    ) -> Result<Option<Box<dyn crate::NativeAdmission>>, paperpaw_sdk::PluginProblem> {
        let depth = crate::native_host::flat::FlatDepth::try_new().unwrap();
        let admission = crate::native_host::hosted_admission::HostedAdmission::new(
            super::native_screen_install::host(0),
            depth,
            |_, _, _, _| {},
            |_, _| {},
        );
        Ok(Some(Box::new(admission)))
    }
}

#[test]
fn a_card_plugin_whose_make_the_shell_refuses_is_listed_failed_and_nothing_stays() {
    // Not announced by the test: `HostedAdmission::new` does it.
    reset(NORMAL);
    REFUSE_ENTRY.with(|refuse| refuse.set(true));
    let storage = paperpaw_sdk::MemoryStorage::new()
        .with(
            "plugins/factory/plugin.ini",
            "[plugin]\nname = Factory\nversion = 1.0.0\n",
        )
        .with("plugins/factory/code.ppn", "PPNP");
    assert!(paperpaw_sdk::native::remember_code(
        &storage, "factory", &[8; 32]
    ));
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .storage(storage)
        .code_plugins(
            paperpaw_sdk::native::Native::new("code.ppn"),
            Box::new(HostedLoader),
        )
        .build()
        .unwrap();
    let info = kernel.plugins().get("factory").unwrap();
    assert_eq!(info.state, paperpaw_sdk::PluginState::Failed);
    assert!(info.problem.is_some(), "the row says why");
    assert!(
        !kernel
            .shared
            .registry
            .screens
            .contains_key("factory.screen")
    );
    assert!(kernel.shared.native_instances.find("factory").is_none());
    // Announced once, refused: no plugin code ran, nothing was left open.
    assert_eq!(counts(), (1, 0));
}
