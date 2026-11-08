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

#[test]
fn a_priority_outside_i16_counts_as_the_nearest_end_and_the_plugin_installs() {
    let mut kernel = fresh();
    install_resources(&mut kernel, 9, FOLDER).unwrap();
    for slot in ["core.translate", "core.lookup"] {
        assert!(has_slot(&kernel, slot), "{slot}");
    }
    let priorities: Vec<_> = kernel
        .shared
        .registry
        .offer_priorities()
        .map(|priority| priority.0)
        .collect();
    // The kernel's own offers come first; the plugin's were last made.
    assert!(priorities.ends_with(&[i16::MAX, i16::MIN]), "{priorities:?}");
}

/// A plugin of the program offering `core.translate` at `priority`.
struct Other(i16);
impl paperpaw_sdk::Plugin for Other {
    fn manifest(&self) -> paperpaw_sdk::Manifest {
        paperpaw_sdk::Manifest::new(
            Id::new_static("other"),
            "Other",
            paperpaw_sdk::Version::new(1, 0, 0),
        )
        .uses(paperpaw_sdk::Uses::NOTHING)
    }
    fn register(&self, r: &mut paperpaw_sdk::Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
        struct Empty;
        impl paperpaw_sdk::Screen for Empty {
            fn draw(&mut self, _: &mut paperpaw_graphics::Canvas<'_>, _: &paperpaw_sdk::Env<'_>) {}
            fn on_action(
                &mut self,
                _: &Id,
                _: &mut paperpaw_sdk::Cx<'_, '_>,
            ) -> paperpaw_sdk::Handled {
                paperpaw_sdk::Handled::No
            }
        }
        r.add_screen(paperpaw_sdk::ScreenSpec::new(
            Id::new_static("other.screen"),
            "other.title",
            Box::new(|_, _| Box::new(Empty)),
        ))?;
        r.bind_slot(
            Id::new_static("core.translate"),
            Id::new_static("other.screen"),
            paperpaw_sdk::Priority(self.0),
        )
    }
}

#[test]
fn two_plugins_offering_one_slot_leave_the_other_when_one_unloads() {
    // The native offer is at priority 4: the other plugin's wins at 10, and
    // is the one left at -5 when the native plugin goes.
    for (priority, first) in [(10, "other.screen"), (-5, "factory.screen")] {
        reset(NORMAL);
        let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
            .plugin(Other(priority))
            .build()
            .unwrap();
        install_resources(&mut kernel, 8, FOLDER).unwrap();
        let env = kernel.shared.env(paperpaw_platform::Instant(0));
        assert_eq!(
            env.slot_screen("core.translate").map(Id::as_str),
            Some(first)
        );
        assert!(kernel.unload_plugin(&Id::new_static("factory")));
        let env = kernel.shared.env(paperpaw_platform::Instant(0));
        assert_eq!(
            env.slot_screen("core.translate").map(Id::as_str),
            Some("other.screen")
        );
        assert!(!env.has_slot(&Id::new_static("core.lookup")));
    }
}

#[test]
fn the_list_that_stays_when_it_cannot_be_made_anew_drops_slots_without_an_offer() {
    let mut kernel = fresh();
    install_resources(&mut kernel, 8, FOLDER).unwrap();
    let mut slots = core::mem::take(&mut kernel.shared.slots);
    assert!(slots.iter().any(|slot| slot.as_str() == "core.translate"));
    kernel.shared.registry.retain_bound(&mut slots);
    assert!(slots.iter().any(|slot| slot.as_str() == "core.translate"));
    kernel.shared.registry.remove_native_offers("factory");
    kernel.shared.registry.retain_bound(&mut slots);
    for slot in ["core.translate", "core.lookup", "factory.own"] {
        assert!(!slots.iter().any(|s| s.as_str() == slot), "{slot}");
    }
}
