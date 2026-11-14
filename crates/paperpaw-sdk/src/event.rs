//! Events: notifications the kernel and plugins emit, which every
//! subscriber hears in registration order, then every open screen.
//!
//! An [`Event<P>`] names an event and the type of what it carries, so
//! emitting and hearing it are checked as the plugin is built:
//!
//! ```
//! use paperpaw_sdk::event::Event;
//!
//! paperpaw_sdk::event_data! {
//!     /// A word was saved.
//!     pub struct Saved {
//!         /// The word.
//!         pub word: String,
//!         /// How often it was saved before.
//!         pub count: u32,
//!     }
//! }
//!
//! pub const SAVED: Event<Saved> = Event::new("demo.saved");
//!
//! // Emitting: out.push(SAVED.emit(Saved::new(word, count)));
//! // Hearing:  r.on(SAVED, |env, saved, out| { /* saved.word */ })?;
//! // On a screen: if let Some(saved) = SAVED.read(event, params) { … }
//! ```
//!
//! Underneath, what an event carries travels as [`Params`] (named
//! strings), so it can cross to plugins in other languages and be
//! declared as work ([`work`](crate::work)); events of other plugins, known only
//! at run time, are heard by id ([`Registrar::subscribe`](crate::Registrar::subscribe)).

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use core::fmt;
use core::marker::PhantomData;

use crate::command::Command;
use crate::env::Env;
use crate::id::Id;
use crate::params::Params;

/// Reacts to an event by emitting commands.
///
/// Events are notifications: the kernel and plugins emit them with
/// [`Command::Emit`], and every subscriber is called in registration order.
/// Any plugin can define its own events (namespaced ids such as
/// `reader.page-turned`), which other plugins may subscribe to.
pub type EventHandler = Box<dyn Fn(&Env<'_>, &Params, &mut crate::Commands)>;

/// A subscription kept in the program, in a `static` list of them
/// ([`Registrar::add_subscriptions`](crate::Registrar::add_subscriptions)):
/// it takes next to no memory, where each added with
/// [`Registrar::on`](crate::Registrar::on) or
/// [`subscribe`](crate::Registrar::subscribe) takes some thirty bytes.
/// Its handler reads what the event carries itself (e.g.
/// [`Event::read`]):
///
/// ```
/// # use paperpaw_sdk::{builtin::events, event::FixedSubscription};
/// static HEARS: [FixedSubscription; 1] = [FixedSubscription::new(
///     events::STARTED.id_static(),
///     |_env, _params, _out| {},
/// )];
/// ```
#[non_exhaustive]
pub struct FixedSubscription {
    event: Id,
    handler: fn(&Env<'_>, &Params, &mut crate::Commands),
}

impl FixedSubscription {
    /// `handler` hears `event`.
    pub const fn new(event: Id, handler: fn(&Env<'_>, &Params, &mut crate::Commands)) -> Self {
        Self { event, handler }
    }

    pub fn event(&self) -> &Id {
        &self.event
    }

    /// Calls its handler.
    pub fn call(&self, env: &Env<'_>, params: &Params, out: &mut crate::Commands) {
        (self.handler)(env, params, out);
    }
}

impl core::fmt::Debug for FixedSubscription {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FixedSubscription")
            .field("event", &self.event)
            .finish_non_exhaustive()
    }
}

/// What an event carries, written as and read from [`Params`]; made with
/// [`event_data!`](crate::event_data) for structs of named values.
pub trait EventData: Clone + Sized + 'static {
    fn to_params(&self) -> Params;
    /// `None` if a value it needs is missing or unreadable.
    fn from_params(params: &Params) -> Option<Self>;
    /// What an event carries when it is emitted: as written by
    /// [`to_params`](Self::to_params), unless the data is too large to
    /// copy (an HTTP answer's body); then it may go along as it is
    /// ([`Params::carrying`]), and a hearer of its type reads it without
    /// a copy.
    fn into_params(self) -> Params {
        self.to_params()
    }
}

/// Nothing: for events that only say that something happened.
impl EventData for () {
    fn to_params(&self) -> Params {
        Params::new()
    }

    fn from_params(_: &Params) -> Option<Self> {
        Some(())
    }
}

/// Named strings as they are, for events whose values vary.
impl EventData for Params {
    fn to_params(&self) -> Params {
        self.clone()
    }

    fn from_params(params: &Params) -> Option<Self> {
        Some(params.clone())
    }
}

/// A value an event carries, as text.
pub trait ParamValue: Sized {
    fn to_param(&self) -> String;
    fn from_param(text: &str) -> Option<Self>;
    /// The value when the event does not carry it: `None` makes it
    /// required (the event is not heard without it).
    fn missing() -> Option<Self> {
        None
    }
}

impl ParamValue for String {
    fn to_param(&self) -> String {
        self.clone()
    }

    fn from_param(text: &str) -> Option<Self> {
        Some(String::from(text))
    }
}

impl ParamValue for Id {
    fn to_param(&self) -> String {
        self.as_str().to_string()
    }

    fn from_param(text: &str) -> Option<Self> {
        Id::parse(text).ok()
    }
}

impl ParamValue for bool {
    fn to_param(&self) -> String {
        String::from(if *self { "true" } else { "false" })
    }

    fn from_param(text: &str) -> Option<Self> {
        match text {
            "true" | "1" | "on" | "yes" => Some(true),
            "false" | "0" | "off" | "no" => Some(false),
            _ => None,
        }
    }
}

macro_rules! numbers {
    ($($t:ty),*) => {$(
        impl ParamValue for $t {
            fn to_param(&self) -> String {
                self.to_string()
            }

            fn from_param(text: &str) -> Option<Self> {
                text.trim().parse().ok()
            }
        }
    )*};
}
numbers!(u8, u16, u32, u64, usize, i32, i64);

/// Written and read without `core`'s formatter and parser of floats:
/// those are some 36 KB of the firmware (#258) and 16 KB of a native
/// plugin taking a reading event (ADR 0024). Written as `Display` writes
/// it; read as near as the float can say (within a unit of its last
/// place).
impl ParamValue for f32 {
    fn to_param(&self) -> String {
        let mut text = String::new();
        // A string takes what is written.
        let _ = crate::float_text::write_f32(&mut text, *self);
        text
    }

    fn from_param(text: &str) -> Option<Self> {
        crate::float_text::parse_f32(text.trim())
    }
}

/// An optional value: `None` when the event does not carry it.
impl<T: ParamValue> ParamValue for Option<T> {
    fn to_param(&self) -> String {
        self.as_ref().map(T::to_param).unwrap_or_default()
    }

    fn from_param(text: &str) -> Option<Self> {
        Some(T::from_param(text))
    }

    fn missing() -> Option<Self> {
        Some(None)
    }
}

/// Writes `value` under `key` unless it is an absent optional one (for
/// [`event_data!`](crate::event_data)).
#[doc(hidden)]
pub fn put<T: ParamValue>(params: &mut Params, key: &'static str, value: &T) {
    let text = value.to_param();
    if !(text.is_empty() && T::missing().is_some()) {
        params.set(key, text);
    }
}

/// Reads the value under `key` (for [`event_data!`](crate::event_data)).
#[doc(hidden)]
pub fn take<T: ParamValue>(params: &Params, key: &str) -> Option<T> {
    match params.get(key) {
        Some(text) => T::from_param(text),
        None => T::missing(),
    }
}

/// A struct of named values an event carries, each a [`ParamValue`]
/// (`String`, `Id`, numbers, `bool`, or an `Option` of one); each is
/// carried under its field's name. See the [module documentation](self).
///
/// The struct is `#[non_exhaustive]` and made with `new`, from its fields
/// in their order. Fields in an `optional` block after it are `Option`s,
/// `None` from `new` and set with a method of their name; a field added
/// there later breaks no one who emits the event:
///
/// ```
/// paperpaw_sdk::event_data! {
///     /// A page was read.
///     pub struct Read {
///         pub path: String,
///     }
///     optional {
///         /// Its number, where it is known.
///         pub page: u32,
///     }
/// }
///
/// let read = Read::new(String::from("books/a.epub")).page(12);
/// assert_eq!(read.page, Some(12));
/// ```
/// A [`FixedSubscription`] of `event` (an [`Event`] constant) calling
/// `handler` with what it carries, as [`Registrar::on`](crate::Registrar::on)
/// would; `handler` a function or a closure that captures nothing:
///
/// ```
/// # use paperpaw_sdk::{builtin::events, hears, FixedSubscription};
/// static HEARS: [FixedSubscription; 1] = [hears!(events::STARTED, |_env, _, _out| {})];
/// ```
#[macro_export]
macro_rules! hears {
    ($event:expr, $handler:expr $(,)?) => {
        $crate::FixedSubscription::new($event.id_static(), |env, params, out| {
            $event.hear(env, params, out, $handler)
        })
    };
}

#[macro_export]
macro_rules! event_data {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$field_meta:meta])*
                $field_vis:vis $field:ident : $type:ty
            ),* $(,)?
        }
        $(
            optional {
                $(
                    $(#[$opt_meta:meta])*
                    $opt_vis:vis $opt:ident : $opt_type:ty
                ),* $(,)?
            }
        )?
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq)]
        #[non_exhaustive]
        $vis struct $name {
            $(
                $(#[$field_meta])*
                $field_vis $field: $type,
            )*
            $($(
                $(#[$opt_meta])*
                $opt_vis $opt: Option<$opt_type>,
            )*)?
        }

        impl $name {
            /// Made of its fields in their order; the optional ones are
            /// `None`.
            #[allow(clippy::too_many_arguments, dead_code, reason = "made by the macro for every event: as many fields as it has, and not every event's used")]
            $vis fn new($($field: $type),*) -> Self {
                Self {
                    $( $field, )*
                    $($( $opt: None, )*)?
                }
            }

            $($(
                /// Sets this optional field (to `None` too).
                #[allow(dead_code, reason = "made by the macro for every event's optional fields; not every one is used")]
                $opt_vis fn $opt(mut self, $opt: impl Into<Option<$opt_type>>) -> Self {
                    self.$opt = $opt.into();
                    self
                }
            )*)?
        }

        impl $crate::event::EventData for $name {
            fn to_params(&self) -> $crate::Params {
                let mut params = $crate::Params::new();
                $( $crate::event::put(&mut params, stringify!($field), &self.$field); )*
                $($( $crate::event::put(&mut params, stringify!($opt), &self.$opt); )*)?
                params
            }

            fn from_params(params: &$crate::Params) -> Option<Self> {
                Some(Self {
                    $( $field: $crate::event::take(params, stringify!($field))?, )*
                    $($( $opt: $crate::event::take(params, stringify!($opt))?, )*)?
                })
            }
        }
    };
}

/// An event: its id and what it carries (see the [module
/// documentation](self)).
pub struct Event<P: EventData = ()> {
    id: Id,
    data: PhantomData<fn() -> P>,
}

impl<P: EventData> Event<P> {
    /// The event `id`, e.g. `reader.page-turned`; checked as for
    /// [`Id::new_static`].
    pub const fn new(id: &'static str) -> Self {
        Self {
            id: Id::new_static(id),
            data: PhantomData,
        }
    }

    pub const fn id(&self) -> &Id {
        &self.id
    }

    /// Its id, kept in the program: for `static` lists, e.g. of
    /// [`FixedSubscription`]s. Takes the event, so it can be used in
    /// `const` and `static` items.
    pub const fn id_static(self) -> Id {
        let id = Id::new_static(self.id.static_text());
        core::mem::forget(self);
        id
    }

    pub fn as_str(&self) -> &str {
        self.id.as_str()
    }

    /// The command that emits the event with `data`.
    pub fn emit(&self, data: P) -> Command {
        Command::Emit(self.id.clone(), data.into_params())
    }

    /// The command that emits it with `data` at `at`: work at a time
    /// ([`Work::at`](crate::work::Work::at)), in place of the one before.
    pub fn schedule(&self, data: P, at: paperpaw_platform::Instant) -> Command {
        Command::work(crate::work::Work::at(self.id.clone(), at).with(data.to_params()))
    }

    /// Calls `handler` with what `params` carry, without a copy where they
    /// carry it as it is ([`Registrar::on`](crate::Registrar::on) does the
    /// same): for handlers kept in the program ([`FixedSubscription`],
    /// [`hears!`](crate::hears)). Not called where they lack what it
    /// needs.
    pub fn hear(
        &self,
        env: &Env<'_>,
        params: &Params,
        out: &mut crate::Commands,
        handler: impl FnOnce(&Env<'_>, &P, &mut crate::Commands),
    ) {
        match params.carried::<P>() {
            Some(data) => handler(env, data, out),
            None => {
                if let Some(data) = P::from_params(params) {
                    handler(env, &data, out);
                }
            }
        }
    }

    /// What it carries, if `event` is this event (for screens'
    /// [`on_event`](crate::Screen::on_event)).
    pub fn read(&self, event: &Id, params: &Params) -> Option<P> {
        if *event != self.id {
            return None;
        }
        match params.carried::<P>() {
            Some(data) => Some(data.clone()),
            None => P::from_params(params),
        }
    }
}

impl<P: EventData> Clone for Event<P> {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            data: PhantomData,
        }
    }
}

impl<P: EventData> fmt::Debug for Event<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Event({})", self.id)
    }
}

impl<P: EventData> AsRef<str> for Event<P> {
    fn as_ref(&self) -> &str {
        self.id.as_str()
    }
}

impl<P: EventData> From<Event<P>> for Id {
    fn from(event: Event<P>) -> Id {
        event.id
    }
}

impl<P: EventData> From<&Event<P>> for Id {
    fn from(event: &Event<P>) -> Id {
        event.id.clone()
    }
}

impl<P: EventData> PartialEq<Id> for Event<P> {
    fn eq(&self, other: &Id) -> bool {
        self.id == *other
    }
}

impl<P: EventData> PartialEq for Event<P> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<P: EventData> Eq for Event<P> {}

impl<P: EventData> PartialEq<Event<P>> for Id {
    fn eq(&self, other: &Event<P>) -> bool {
        *self == other.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::event_data! {
        struct Moved {
            from: String,
            to: String,
            count: u32,
            note: Option<String>,
            done: bool,
        }
    }

    const MOVED: Event<Moved> = Event::new("demo.moved");

    #[test]
    fn data_goes_through_params_and_back() {
        let moved = Moved {
            from: "a b.epub".into(),
            to: "c.epub".into(),
            count: 3,
            note: None,
            done: true,
        };
        let Command::Emit(id, params) = MOVED.emit(moved.clone()) else {
            panic!("an emit");
        };
        assert_eq!(id, MOVED);
        assert_eq!(params.get("from"), Some("a b.epub"));
        assert_eq!(params.get("note"), None, "absent, not empty");
        assert_eq!(MOVED.read(&id, &params), Some(moved));
        // Another event, or one without what is required: not this one.
        assert_eq!(MOVED.read(&Id::new_static("demo.other"), &params), None);
        assert_eq!(MOVED.read(&id, &Params::new().with("from", "x")), None);
        let with_note = params.clone().with("note", "hi");
        assert_eq!(
            MOVED.read(&id, &with_note).and_then(|m| m.note),
            Some(String::from("hi"))
        );
    }

    #[test]
    fn floats_read_as_near_as_they_can_be() {
        for value in [
            0.0f32,
            0.5,
            1.0,
            0.25,
            0.333_333_34,
            0.999_999_9,
            1.5e-7,
            12_345.678,
            3.0e9,
            7.0e-30,
            f32::MAX,
            f32::MIN_POSITIVE,
        ] {
            for v in [value, -value] {
                let read = f32::from_param(&v.to_param()).unwrap();
                let diff = (read - v).abs();
                assert!(diff <= v.abs() * 2.5e-7, "{v}: read {read}");
            }
        }
        assert_eq!(f32::from_param("1e3"), Some(1000.0));
        assert_eq!(f32::from_param(" .5 "), Some(0.5));
        assert_eq!(f32::from_param("5."), Some(5.0));
        assert_eq!(f32::from_param("inf"), Some(f32::INFINITY));
        assert_eq!(f32::from_param("-inf"), Some(f32::NEG_INFINITY));
        assert!(f32::from_param("NaN").unwrap().is_nan());
        for bad in ["", "-", ".", "1.2.3", "1e", "abc", "1,5", "--1", "1e3.5"] {
            assert_eq!(f32::from_param(bad), None, "{bad:?}");
        }
    }
}
