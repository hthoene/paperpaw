//! Services and introspection: services by contract and
//! version, events declared, the screens of slots, the
//! bindings of a layer with their origin.

use super::*;
use crate::Items;
use paperpaw_sdk::keys::{BindingOrigin, KeyBindings};
use paperpaw_sdk::services::EventSpec;

/// A service with a contract.
struct Forecast(&'static str);

paperpaw_sdk::service_type!(Forecast, "weather.forecast/1");

/// Another type that claims the same contract, of another size.
struct Pretender(u64, u64, u64);

paperpaw_sdk::service_type!(Pretender, "weather.forecast/1");

fn weather() -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    |r| {
        r.provide(
            Id::new_static("weather.today"),
            Forecast("sunny"),
            Version::new(1, 2, 0),
        )?;
        r.declare_event(
            EventSpec::new(Id::new_static("weather.changed"), "weather.changed").params(&["sky"]),
        )?;
        r.add_keymap(Cow::Borrowed(
            "[keymap.weather.screen]\nbutton.menu = core.home\n",
        ))
    }
}

/// What [`weather`] registers under.
fn weather_manifest() -> Manifest {
    Manifest::new(Id::new_static("weather"), "Weather", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

/// A service with a contract is found by it and by the version needed;
/// another type claiming the contract is not; events declared
/// are listed.
#[test]
fn services_by_contract_and_version() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(TestPlugin::new("a", &log))
        .items(weather_manifest(), weather())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let services = &kernel.shared.services;
    let today = services.get::<Forecast>("weather.today", Version::new(1, 0, 0));
    assert_eq!(today.map(|f| f.0), Some("sunny"));
    let fits = services.get::<Forecast>("weather.today", Version::new(1, 1, 0));
    assert!(fits.is_some());
    assert!(
        services
            .get::<Forecast>("weather.today", Version::new(1, 3, 0))
            .is_none()
    );
    assert!(
        services
            .get::<Forecast>("weather.today", Version::new(2, 0, 0))
            .is_none()
    );
    assert!(
        services
            .get::<Pretender>("weather.today", Version::new(1, 0, 0))
            .is_none()
    );
    let pretender = Pretender(0, 0, 0);
    assert_eq!(pretender.0 + pretender.1 + pretender.2, 0);
    assert!(services.ids().any(|id| id.as_str() == "weather.today"));
    assert_eq!(services.events()[0].params, ["sky"]);
}

/// The screen a slot shows and every screen offered for it; the bindings
/// of a layer, each with where it comes from.
#[test]
fn slots_and_bindings_are_readable() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(TestPlugin::new("a", &log))
        .test_plugin(TestPlugin {
            priority: Priority(10),
            ..TestPlugin::new("b", &log)
        })
        .items(weather_manifest(), weather())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.apply(Command::bind(
        "weather.screen",
        "long.menu",
        Some(actions::BACK),
    ));
    kernel.finish();
    let env = kernel.shared.place(Instant(0));
    assert_eq!(
        env.slot_screen(slots::HOME.as_str()).map(Id::as_str),
        Some("b.screen")
    );
    let offered: Vec<&str> = env
        .slot_screens(slots::HOME.as_str())
        .into_iter()
        .map(Id::as_str)
        .collect();
    // The highest priority first: the kernel's own placeholder last.
    assert_eq!(offered, ["b.screen", "a.screen", "core.placeholder"]);
    let mut bindings = kernel.shared.keymap.bindings("weather.screen");
    bindings.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        bindings,
        [
            (
                String::from("button.menu"),
                Some(actions::HOME),
                BindingOrigin::Plugin
            ),
            (
                String::from("long.menu"),
                Some(actions::BACK),
                BindingOrigin::User
            ),
        ]
    );
}
