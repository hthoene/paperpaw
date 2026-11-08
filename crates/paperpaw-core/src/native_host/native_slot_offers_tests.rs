//! A native plugin's `slot.offer` reaches the registry the kernel opens slots
//! by, so a plugin that asks `Env::has_slot` sees it; it goes with the plugin.
use super::*;

const FOLDER: Option<&str> = None;

fn fresh() -> crate::Kernel {
    reset(NORMAL);
    DESTROYED.with(|value| value.set(0));
    crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap()
}

fn has_slot(kernel: &crate::Kernel, slot: &'static str) -> bool {
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    env.has_slot(&Id::new_static(slot))
}

#[test]
fn an_offered_slot_is_bound_and_goes_with_the_plugin() {
    let mut kernel = fresh();
    let offered = kernel.shared.registry.offers();
    assert!(!has_slot(&kernel, "core.translate"));
    install_resources(&mut kernel, 8, FOLDER).unwrap();
    for slot in ["core.translate", "core.lookup", "factory.own"] {
        assert!(has_slot(&kernel, slot), "{slot}");
        let env = kernel.shared.env(paperpaw_platform::Instant(0));
        assert_eq!(
            env.slot_screen(slot).map(Id::as_str),
            Some("factory.screen"),
            "{slot}"
        );
    }
    assert!(kernel.unload_plugin(&Id::new_static("factory")));
    for slot in ["core.translate", "core.lookup", "factory.own"] {
        assert!(!has_slot(&kernel, slot), "{slot}");
    }
    assert_eq!(kernel.shared.registry.offers(), offered);
}

#[test]
fn every_allocation_of_an_install_with_offers_refuses_whole() {
    let mut reached_success = false;
    let offered = fresh().shared.registry.offers();
    for allocation in 1..600 {
        let mut kernel = fresh();
        INSTALL_FAIL_NTH.with(|value| value.set(allocation));
        let result = admit_resources(&mut kernel, 8, FOLDER);
        arm_failure(0);
        INSTALL_FAIL_NTH.with(|value| value.set(0));
        if result.is_ok() {
            reached_success = true;
            assert_eq!(kernel.shared.registry.offers(), offered + 3);
            break;
        }
        assert_eq!(kernel.shared.registry.offers(), offered, "{allocation}");
        assert!(
            !kernel
                .shared
                .registry
                .screens
                .contains_key("factory.screen")
        );
        assert_eq!(DESTROYED.with(Cell::get), 1, "{allocation}");
    }
    assert!(reached_success, "the fault walk reached an admitted owner");
}
