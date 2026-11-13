//! `HostedAdmission`: a hosted plugin, not made yet, admitted by the kernel
//! with everything it declares (`CodeLoader::load_native`), the same on every
//! shell.
use super::native_screen_install::{ARMS, host};
use super::*;
use crate::NativeAdmission;
use crate::native_host::hosted_admission::{HostedAdmission, problem_of};
use core::cell::RefCell;
use paperpaw_sdk::PluginProblem;
use std::string::String;
use std::vec::Vec;

std::thread_local! {
    static REFUSED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// A kernel with nothing admitted yet, and the admission of a plugin that
/// declares what `arms` says (`ARMS`: 1 a screen, a group, a setting and an
/// action; 2 also a family this host does not map).
fn admission(arms: u8) -> (crate::Kernel, Box<dyn NativeAdmission>) {
    reset(NORMAL);
    REFUSED.with(|refused| refused.borrow_mut().clear());
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    // The device-wide counter is the first plugin's: made here as a shell does.
    let depth = crate::native_host::flat::FlatDepth::try_new().unwrap();
    ARMS.with(|value| value.set(arms));
    let hosted = host(0);
    ARMS.with(|value| value.set(0));
    let admission = HostedAdmission::new(
        hosted,
        depth,
        |_, _, _, _| {},
        |folder, why| {
            REFUSED.with(|refused| refused.borrow_mut().push(std::format!("{folder}: {why:?}")))
        },
    );
    (kernel, Box::new(admission))
}

#[test]
fn a_hosted_plugin_is_admitted_with_what_it_declares() {
    let (mut kernel, admission) = admission(1);
    // `make` runs in the kernel's transaction: the plugin is not made before.
    ARMS.with(|value| value.set(1));
    let result = admission.admit(&mut kernel, "factory");
    ARMS.with(|value| value.set(0));
    result.unwrap();
    let shared = &kernel.shared;
    assert!(shared.registry.screens.contains_key("factory.screen"));
    assert!(shared.actions.contains("factory.action"));
    assert_eq!(
        shared.settings.spec("factory.flag").unwrap().title,
        "factory.flag"
    );
    assert!(shared.native_instances.find("factory").is_some());
    assert!(REFUSED.with(|refused| refused.borrow().is_empty()));
}

#[test]
fn a_plugin_the_host_cannot_serve_whole_is_refused_and_nothing_of_it_stays() {
    let (mut kernel, admission) = admission(2);
    ARMS.with(|value| value.set(2));
    let result = admission.admit(&mut kernel, "factory");
    ARMS.with(|value| value.set(0));
    assert_eq!(result, Err(PluginProblem::NeedsNewerPaperPaw));
    let shared = &kernel.shared;
    assert!(!shared.registry.screens.contains_key("factory.screen"));
    assert!(!shared.actions.contains("factory.action"));
    assert!(shared.settings.spec("factory.flag").is_none());
    assert!(shared.native_instances.find("factory").is_none());
    // The shell is told why, with the folder.
    let refused = REFUSED.with(|refused| refused.borrow().clone());
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(refused[0].starts_with("factory: "), "{refused:?}");
}

#[test]
fn a_refusal_is_said_the_same_way_on_every_shell() {
    use crate::native_host::shell::Error;
    use host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_UNSUPPORTED};
    let said = |error: Error| problem_of(&error);
    assert_eq!(said(Error::Allocation), PluginProblem::NoRoom);
    assert_eq!(said(Error::Status(ERR_NO_MEMORY)), PluginProblem::NoRoom);
    assert_eq!(
        said(Error::Status(ERR_UNSUPPORTED)),
        PluginProblem::NeedsNewerPaperPaw
    );
    assert_eq!(said(Error::Status(ERR_INVALID)), PluginProblem::Damaged);
    assert_eq!(said(Error::Refused), PluginProblem::NoRoom);
    assert_eq!(said(Error::Factory(-1)), PluginProblem::Damaged);
    assert_eq!(
        said(Error::Lifecycle(crate::native_host::Error::Memory)),
        PluginProblem::NoRoom
    );
}
