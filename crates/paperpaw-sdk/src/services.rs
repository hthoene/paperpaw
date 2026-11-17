//! What plugins offer each other: services, and points to contribute to.
//!
//! **Services.** A plugin provides a value under an id in its namespace
//! (`Items::provide`), in a version;
//! others find it with [`Services::get`] by that id, its type and the
//! version they need. The type comes from the providing plugin's crate,
//! usually a struct holding a trait object, so others depend on what it
//! promises, not on how it does it. Its contract names it in a key that
//! stays the same in every build ([`ServiceType`], written with
//! [`service_type!`](crate::service_type)): a native code plugin (see
//! [`native`](crate::native)) is built apart from the firmware, so the
//! same type has another `TypeId` there, and is found by its key, its
//! size and alignment checked.
//!
//! ```
//! # use paperpaw_sdk::services::{Service, Services};
//! # use paperpaw_sdk::{Id, Version};
//! # extern crate alloc;
//! # use alloc::boxed::Box;
//! /// In the providing plugin's crate.
//! pub trait Forecast {
//!     fn today(&self) -> &str;
//! }
//! pub struct Weather(pub Box<dyn Forecast>);
//! paperpaw_sdk::service_type!(Weather, "weather.forecast/1");
//! pub const WEATHER: Id = Id::new_static("weather.service");
//!
//! struct Sunny;
//! impl Forecast for Sunny {
//!     fn today(&self) -> &str {
//!         "sunny"
//!     }
//! }
//!
//! // `register` of the providing plugin:
//! // r.provide(WEATHER, Weather(Box::new(Sunny)), Version::new(1, 0, 0))?;
//! # let mut services = Services::new();
//! # services.insert(WEATHER, Service::new(Weather(Box::new(Sunny)), Version::new(1, 0, 0)));
//!
//! // Any plugin, with an `Env`: `env.services().get::<…>(…)`.
//! let weather = services.get::<Weather>(WEATHER.as_str(), Version::new(1, 0, 0));
//! assert_eq!(weather.map(|w| w.0.today()), Some("sunny"));
//! ```
//!
//! A service is shared by all and lives as long as PaperPaw runs; one
//! that changes keeps its state in a `Cell` or `RefCell`. One that is
//! missing (the plugin is switched off) is `None`: plugins that use
//! another's service do without it.
//!
//! **Points to contribute to.** A plugin names a point (an id in its
//! namespace) and documents what a contribution there is, e.g. a line
//! of text in a format of its own; any plugin adds to it with
//! `Items::contribute`, and the
//! plugin reads them all with [`Services::contributions`]. The kernel's
//! own points (status items, widgets, text filters …) have registrar
//! methods of their own; these are for points that plugins define.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::any::Any;
use core::fmt;

use crate::{Id, Version};

/// The contract of a service type: a key that names it in every build
/// (`"<plugin>.<name>/<major>"`), so that plugins built apart find it.
/// Implemented by the type that is provided, usually a struct holding a
/// trait object, with [`service_type!`](crate::service_type):
///
/// ```
/// # extern crate alloc;
/// pub trait Forecast {
///     fn today(&self) -> &str;
/// }
/// pub struct Weather(pub alloc::boxed::Box<dyn Forecast>);
/// paperpaw_sdk::service_type!(Weather, "weather.forecast/1");
/// ```
///
/// # Safety
///
/// Every type that names this key, in any build, is the same type: the
/// same fields of the same types, so that a value one build made another
/// reads as its own. A type whose fields change takes a new key (`/2`).
/// The size and alignment are checked; the rest is this promise.
pub unsafe trait ServiceType: Any {
    const KEY: &'static str;
}

/// Makes `$type` a service type with the contract `$key`
/// ([`ServiceType`]): the key is the promise that every type naming it
/// is this one.
#[macro_export]
macro_rules! service_type {
    ($type:ty, $key:literal) => {
        // SAFETY: the key names this type; one whose fields change takes
        // a new key (see `ServiceType`).
        unsafe impl $crate::services::ServiceType for $type {
            const KEY: &'static str = $key;
        }
    };
}

/// A service's value, with its contract: its type's key, size and
/// alignment, and the version it was provided in.
pub struct Service {
    value: Box<dyn Any>,
    contract: Contract,
}

/// A service's contract: its type's key, size and alignment, and the
/// version it was provided in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Contract {
    key: &'static str,
    size: usize,
    align: usize,
    version: Version,
}

impl Service {
    /// `value` of a type with a contract ([`ServiceType`]) in `version`:
    /// found by its `TypeId`, or by its contract's key where its size and
    /// alignment are the same.
    pub fn new<T: ServiceType>(value: T, version: Version) -> Self {
        Self {
            value: Box::new(value),
            contract: Contract {
                key: T::KEY,
                size: core::mem::size_of::<T>(),
                align: core::mem::align_of::<T>(),
                version,
            },
        }
    }

    /// The version it was provided in.
    pub fn version(&self) -> Version {
        self.contract.version
    }

    /// The value, if it is a `T` of the same contract.
    fn get<T: ServiceType>(&self) -> Option<&T> {
        if let Some(value) = self.value.downcast_ref::<T>() {
            return Some(value);
        }
        let c = &self.contract;
        if (c.key, c.size, c.align)
            != (
                T::KEY,
                core::mem::size_of::<T>(),
                core::mem::align_of::<T>(),
            )
        {
            return None;
        }
        let value: *const dyn Any = &*self.value;
        // SAFETY: the value is of a type with the same contract, size and
        // alignment: the same type, as `ServiceType` promises, only from
        // another build (a native plugin's), which gives it another
        // `TypeId`.
        Some(unsafe { &*value.cast::<T>() })
    }
}

impl fmt::Debug for Service {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Service")
            .field("contract", &self.contract)
            .finish_non_exhaustive()
    }
}

/// A point plugins contribute to, as its plugin declares it
/// (`Items::add_point`): for lists of
/// what can be extended, and for authors.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PointSpec {
    pub id: Id,
    /// Translation key of its name.
    pub title: Cow<'static, str>,
    /// What a contribution there is, its format (for authors).
    pub doc: Cow<'static, str>,
}

impl PointSpec {
    pub fn new(id: Id, title: impl Into<Cow<'static, str>>) -> Self {
        Self {
            id,
            title: title.into(),
            doc: Cow::Borrowed(""),
        }
    }

    pub fn doc(mut self, doc: impl Into<Cow<'static, str>>) -> Self {
        self.doc = doc.into();
        self
    }
}

/// An event a plugin emits, as it declares it
/// (`Items::declare_event`): rules
/// and macro editors offer any declared event.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EventSpec {
    pub id: Id,
    /// Translation key of its name.
    pub title: Cow<'static, str>,
    /// The names of the params it carries.
    pub params: &'static [&'static str],
}

impl EventSpec {
    pub fn new(id: Id, title: impl Into<Cow<'static, str>>) -> Self {
        Self {
            id,
            title: title.into(),
            params: &[],
        }
    }

    pub fn params(mut self, params: &'static [&'static str]) -> Self {
        self.params = params;
        self
    }
}

/// Every service and contribution; see the [module documentation](self).
#[derive(Default)]
pub struct Services {
    services: Vec<(Id, Service)>,
    /// Point, plugin, contribution.
    contributions: Vec<(Id, Id, Cow<'static, str>)>,
    points: Vec<PointSpec>,
    events: Vec<EventSpec>,
}

impl Services {
    pub fn new() -> Self {
        Self::default()
    }

    /// Publishes `service` as `id` (kernel; plugins use
    /// `Items::provide`).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn insert(&mut self, id: Id, service: Service) {
        self.services.push((id, service));
    }

    /// Adds `plugin`'s `contribution` to `point` (kernel; plugins use
    /// `Items::contribute`).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn contribute(&mut self, point: Id, plugin: Id, contribution: Cow<'static, str>) {
        self.contributions.push((point, plugin, contribution));
    }

    /// Declares point `point` (kernel; plugins use
    /// `Items::add_point`).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn declare_point(&mut self, point: PointSpec) {
        self.points.push(point);
    }

    /// Declares event `event` (kernel; plugins use
    /// `Items::declare_event`).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn declare_event(&mut self, event: EventSpec) {
        self.events.push(event);
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.services.shrink_to_fit();
        self.contributions.shrink_to_fit();
        self.points.shrink_to_fit();
        self.events.shrink_to_fit();
    }

    /// Service `id` of contract `T` ([`ServiceType`]) in a version that
    /// satisfies `min` (as a plugin's SDK version does: the same major,
    /// not older), if it is provided.
    pub fn get<T: ServiceType>(&self, id: &str, min: Version) -> Option<&T> {
        self.services
            .iter()
            .find(|(s, service)| s.as_str() == id && service.version().satisfies(&min))
            .and_then(|(_, service)| service.get::<T>())
    }

    /// The ids of every service provided.
    pub fn ids(&self) -> impl Iterator<Item = &Id> {
        self.services.iter().map(|(id, _)| id)
    }

    /// Every point declared to contribute to.
    pub fn points(&self) -> &[PointSpec] {
        &self.points
    }

    /// Every event declared.
    pub fn events(&self) -> &[EventSpec] {
        &self.events
    }

    /// Whether service `id` is provided.
    pub fn has(&self, id: &str) -> bool {
        self.services.iter().any(|(s, _)| s.as_str() == id)
    }

    /// The points something was contributed to, each once, in the order
    /// of their first contribution (declared or not, see
    /// [`Services::points`]).
    pub fn contributed_points(&self) -> impl Iterator<Item = &Id> {
        let all = &self.contributions;
        all.iter()
            .enumerate()
            .filter(move |(at, (point, _, _))| !all[..*at].iter().any(|(p, _, _)| p == point))
            .map(|(_, (point, _, _))| point)
    }

    /// The contributions to `point`, with the plugin each comes from, in
    /// the order the plugins registered.
    pub fn contributions<'a>(&'a self, point: &'a str) -> impl Iterator<Item = (&'a Id, &'a str)> {
        self.contributions
            .iter()
            .filter(move |(p, _, _)| p.as_str() == point)
            .map(|(_, plugin, text)| (plugin, text.as_ref()))
    }
}

impl fmt::Debug for Services {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Services")
            .field(
                "services",
                &self.services.iter().map(|(id, _)| id).collect::<Vec<_>>(),
            )
            .field("contributions", &self.contributions.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    #[repr(C)]
    struct Count(u32);
    crate::service_type!(Count, "a.count/1");

    #[derive(Debug, PartialEq)]
    #[repr(C)]
    struct Name(&'static str);
    crate::service_type!(Name, "b.name/1");

    const ONE: Version = Version::new(1, 0, 0);

    #[test]
    fn services_and_points_are_listed_each_once() {
        let mut services = Services::new();
        services.insert(Id::new_static("a.count"), Service::new(Count(3), ONE));
        services.insert(Id::new_static("b.name"), Service::new(Name("x"), ONE));
        let ids: Vec<&str> = services.ids().map(Id::as_str).collect();
        assert_eq!(ids, ["a.count", "b.name"]);
        let (a, b) = (Id::new_static("a"), Id::new_static("b"));
        services.contribute(Id::new_static("p.one"), a.clone(), Cow::Borrowed("1"));
        services.contribute(Id::new_static("p.two"), b.clone(), Cow::Borrowed("2"));
        services.contribute(Id::new_static("p.one"), b, Cow::Borrowed("3"));
        let points: Vec<&str> = services.contributed_points().map(Id::as_str).collect();
        assert_eq!(points, ["p.one", "p.two"]);
        assert_eq!(Services::new().contributed_points().count(), 0);
    }

    #[test]
    fn services_by_id_type_and_version() {
        let mut services = Services::new();
        services.insert(
            Id::new_static("a.count"),
            Service::new(Count(3), Version::new(1, 2, 0)),
        );
        assert_eq!(services.get::<Count>("a.count", ONE), Some(&Count(3)));
        assert_eq!(services.get::<Name>("a.count", ONE), None, "another type");
        assert_eq!(services.get::<Count>("b.count", ONE), None);
        assert_eq!(
            services.get::<Count>("a.count", Version::new(1, 3, 0)),
            None,
            "too old"
        );
        assert_eq!(
            services.get::<Count>("a.count", Version::new(2, 0, 0)),
            None,
            "another major"
        );
        assert!(services.has("a.count"));

        let point = Id::new_static("a.shapes");
        services.contribute(point.clone(), Id::new_static("b"), Cow::Borrowed("circle"));
        services.contribute(
            Id::new_static("a.other"),
            Id::new_static("b"),
            Cow::Borrowed("x"),
        );
        services.contribute(point, Id::new_static("c"), Cow::Borrowed("square"));
        let shapes: Vec<(&str, &str)> = services
            .contributions("a.shapes")
            .map(|(plugin, text)| (plugin.as_str(), text))
            .collect();
        assert_eq!(shapes, [("b", "circle"), ("c", "square")]);
    }

    /// The same type built apart from the provider: found by its
    /// contract, the key, size and alignment, though its `TypeId` differs.
    #[test]
    fn services_built_apart_are_found_by_their_contract() {
        #[repr(C)]
        struct BuiltApart(u32);
        let mut services = Services::new();
        let apart = Service {
            value: Box::new(BuiltApart(7)),
            contract: Contract {
                key: Count::KEY,
                size: 4,
                align: 4,
                version: ONE,
            },
        };
        services.insert(Id::new_static("w.count"), apart);
        assert_eq!(services.get::<Count>("w.count", ONE), Some(&Count(7)));
        // Another key, or another size, is not the same type.
        assert_eq!(services.get::<Name>("w.count", ONE), None);
    }
}
