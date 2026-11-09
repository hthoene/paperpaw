//! What the Bluetooth plugin shows and decides, free of host calls: the
//! bonded devices and the rows of the page, a search and where adding a
//! device is, and when remotes' keys are switched on.
use alloc::{string::String, vec::Vec};
use core::cmp::Reverse;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::events::{BluetoothDeviceKind, BluetoothReason};
use paperpaw_ui_kit::mem::push;

use crate::declarations::{BY_HAND, REMOTES_CHOICES, WHILE_AWAKE, WHILE_READING};

/// What a device is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Kind {
    Keyboard,
    Remote,
    Sensor,
    Phone,
    #[default]
    Other,
}

impl Kind {
    /// The kind a host row (`dev_bonded.kind`) names; unknown values are `Other`.
    #[must_use]
    pub fn from_wire(kind: u32) -> Self {
        match kind {
            1 => Self::Keyboard,
            2 => Self::Remote,
            3 => Self::Sensor,
            4 => Self::Phone,
            _ => Self::Other,
        }
    }

    /// The kind an event names.
    #[must_use]
    pub fn from_event(kind: BluetoothDeviceKind) -> Self {
        match kind {
            BluetoothDeviceKind::Keyboard => Self::Keyboard,
            BluetoothDeviceKind::Remote => Self::Remote,
            BluetoothDeviceKind::Sensor => Self::Sensor,
            BluetoothDeviceKind::Phone => Self::Phone,
            _ => Self::Other,
        }
    }

    /// Whether its keys can go to the key maps.
    #[must_use]
    pub const fn can_type(self) -> bool {
        matches!(self, Self::Keyboard | Self::Remote)
    }

    /// The key of the kind in words.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Keyboard => "bluetooth.kind.keyboard",
            Self::Remote => "bluetooth.kind.remote",
            Self::Sensor => "bluetooth.kind.sensor",
            Self::Phone => "bluetooth.kind.phone",
            Self::Other => "bluetooth.kind.other",
        }
    }
}

/// A device bonded with this one (`dev_bonded`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bond {
    pub id: u32,
    pub name: String,
    pub kind: Kind,
    pub connected: bool,
    /// Its keys go to the key maps.
    pub keys: bool,
    /// The plugin a sensor is given to; empty for none.
    pub given: String,
}

/// A plugin that uses Bluetooth: a sensor may be given to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    pub id: String,
    pub name: String,
}

/// A row of the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// The bond at this index of the bonds.
    Device(usize),
    /// Gives the bond at this index to the user at that index, or takes it back.
    Give(usize, usize),
    /// Adding a device.
    Add,
    /// When remotes connect.
    Remotes,
}

/// The rows of the page: each device (a sensor with one row for every
/// plugin it may be given to), then adding one and the choice of Remotes.
///
/// # Errors
/// `NoMemory` when a row cannot be allocated.
pub fn rows(bonds: &[Bond], users: &[User]) -> Result<Vec<Row>, Error> {
    let mut rows = Vec::new();
    for (index, bond) in bonds.iter().enumerate() {
        push(&mut rows, Row::Device(index))?;
        if !bond.kind.can_type() {
            for user in 0..users.len() {
                push(&mut rows, Row::Give(index, user))?;
            }
        }
    }
    push(&mut rows, Row::Add)?;
    push(&mut rows, Row::Remotes)?;
    Ok(rows)
}

/// Whether any bonded device may be given to a plugin: only then are the
/// plugins that use Bluetooth read.
#[must_use]
pub fn wants_users(bonds: &[Bond]) -> bool {
    bonds.iter().any(|bond| !bond.kind.can_type())
}

/// When a remote's or a keyboard's keys go to the key maps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Policy {
    /// Only when chosen on the page.
    #[default]
    ByHand,
    /// While a book is open.
    WhileReading,
    /// Whenever the device is awake.
    WhileAwake,
}

impl Policy {
    /// The choice a setting value names; anything else is by hand.
    #[must_use]
    pub fn from_key(key: &str) -> Self {
        match key {
            WHILE_READING => Self::WhileReading,
            WHILE_AWAKE => Self::WhileAwake,
            _ => Self::ByHand,
        }
    }

    /// The key of the choice.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::ByHand => BY_HAND,
            Self::WhileReading => WHILE_READING,
            Self::WhileAwake => WHILE_AWAKE,
        }
    }

    /// The choice after this one, the last going back to the first.
    #[must_use]
    pub fn next(self) -> &'static str {
        let at = REMOTES_CHOICES
            .iter()
            .position(|choice| *choice == self.key())
            .unwrap_or(0);
        REMOTES_CHOICES
            .get((at + 1) % REMOTES_CHOICES.len())
            .copied()
            .unwrap_or(BY_HAND)
    }
}

/// What happened that may change whether remotes' keys are wanted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    /// The reader started or woke.
    Awake,
    BookOpened,
    BookClosed,
}

/// What to ask of the bonded remotes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plan {
    Nothing,
    /// Every remote's keys to the key maps.
    AllOn,
    /// No remote's keys to the key maps.
    AllOff,
    /// The remotes switched on by hand, wanted again: the kernel connects
    /// them, and again whenever they come back.
    SwitchedOn,
}

/// What `edge` asks of the remotes under `policy`, while a book is open
/// (`reading`, after the edge) or not.
#[must_use]
pub fn plan(policy: Policy, edge: Edge, reading: bool) -> Plan {
    match (edge, policy) {
        (Edge::BookOpened, Policy::WhileReading) => Plan::AllOn,
        (Edge::BookClosed, Policy::WhileReading) => Plan::AllOff,
        (Edge::Awake, Policy::WhileAwake) => Plan::AllOn,
        (Edge::Awake, Policy::WhileReading) if reading => Plan::AllOn,
        (Edge::Awake, Policy::ByHand) => Plan::SwitchedOn,
        _ => Plan::Nothing,
    }
}

/// The bonded devices a plan switches, and how.
#[must_use]
pub fn switched(plan: Plan, bond: &Bond) -> Option<bool> {
    if !bond.kind.can_type() {
        return None;
    }
    match plan {
        Plan::Nothing => None,
        Plan::AllOn => Some(true),
        Plan::AllOff => Some(false),
        Plan::SwitchedOn => bond.keys.then_some(true),
    }
}

/// Drops the error of a notice: a notice that cannot be said (no memory, no
/// room in the scratch, a refusal) never undoes what it is about.
pub fn best_effort<T>(notice: Result<T, Error>) {
    match notice {
        Ok(_) | Err(_) => {}
    }
}

/// A device a search found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub id: u32,
    /// Empty where the device did not say.
    pub name: String,
    pub kind: Kind,
    /// In dBm; `None` where not known.
    pub signal: Option<i32>,
    /// Bonded with this one already.
    pub bonded: bool,
}

/// Where adding the chosen device is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Adding {
    Connecting(u32),
    Pairing(u32),
}

/// What went wrong last, said below the search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    /// Bluetooth is off.
    Off,
    /// It did not answer.
    Failed,
    /// The reader declined the pairing.
    Refused,
}

impl Problem {
    /// The key of the words.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Off => "bluetooth.off",
            Self::Failed => "bluetooth.failed",
            Self::Refused => "bluetooth.refused",
        }
    }
}

/// What the page asks of the host after an event of the device being added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Nothing,
    /// Pair with the device that connected.
    Pair(u32),
    /// Paired: its keys go to the key maps if it types; disconnect it; say so; go back.
    Paired {
        id: u32,
        typing: bool,
        name: String,
    },
}

/// What choosing a row of a search does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Search again.
    Search,
    /// Connect this device, then pair it.
    Add(u32),
    Nothing,
}

#[derive(Debug, Default)]
pub struct Search {
    pub found: Vec<Found>,
    pub searching: bool,
    pub adding: Option<Adding>,
    pub problem: Option<Problem>,
}

impl Search {
    /// The rows: what was found, then searching again.
    #[must_use]
    pub fn len(&self) -> usize {
        self.found.len() + 1
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        false
    }

    /// A search starts.
    pub fn start(&mut self) {
        self.searching = true;
        self.problem = None;
    }

    /// What choosing row `index` does. Nothing changes until the command is
    /// queued: then [`Self::begin_add`] or [`Self::start`].
    #[must_use]
    pub fn choose(&self, index: usize) -> Choice {
        if index == self.found.len() {
            return Choice::Search;
        }
        match self.found.get(index) {
            Some(device) if self.adding.is_none() => Choice::Add(device.id),
            _ => Choice::Nothing,
        }
    }

    /// Adding `device` was asked for: connecting.
    pub fn begin_add(&mut self, device: u32) {
        self.problem = None;
        self.adding = Some(Adding::Connecting(device));
    }

    /// A device was found: the strongest first.
    ///
    /// # Errors
    /// `NoMemory` when it cannot be kept.
    pub fn found(&mut self, device: Found) -> Result<(), Error> {
        // Room first: a refused allocation keeps the entry that was there.
        self.found.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.found.retain(|known| known.id != device.id);
        // The list is in order already: the device goes after those as strong
        // (a stable sort would put it there), with the room made above.
        let strength = |found: &Found| Reverse(found.signal.unwrap_or(i32::MIN));
        let place = self
            .found
            .partition_point(|known| strength(known) <= strength(&device));
        self.found.insert(place, device);
        Ok(())
    }

    /// The search ended.
    pub fn ended(&mut self) {
        self.searching = false;
    }

    /// A command failed: `device` is 0 for the search, else the device.
    pub fn failed(&mut self, device: u32, reason: Option<BluetoothReason>) {
        if device == 0 {
            self.searching = false;
            self.problem = Some(match reason {
                Some(BluetoothReason::Off) => Problem::Off,
                _ => Problem::Failed,
            });
        } else if self.adding_id() == Some(device) {
            self.adding = None;
            self.problem = Some(match reason {
                Some(BluetoothReason::Declined) => Problem::Refused,
                _ => Problem::Failed,
            });
        }
    }

    /// The link to `device` ended with a reason of its own.
    pub fn lost(&mut self, device: u32, reason: BluetoothReason) {
        if self.adding_id() == Some(device) {
            self.failed(device, Some(reason));
        }
    }

    /// The device connected.
    pub fn connected(&mut self, device: u32) -> Step {
        match self.adding {
            Some(Adding::Connecting(adding)) if adding == device => {
                self.adding = Some(Adding::Pairing(device));
                Step::Pair(device)
            }
            _ => Step::Nothing,
        }
    }

    /// The device is paired; its `kind` and `name` as the event said.
    ///
    /// # Errors
    /// `NoMemory` when the name cannot be kept.
    pub fn paired(&mut self, device: u32, kind: Kind, name: &str) -> Result<Step, Error> {
        if self.adding_id() != Some(device) {
            return Ok(Step::Nothing);
        }
        self.adding = None;
        let found = self.found.iter().find(|known| known.id == device);
        let typing = found.map_or(kind, |known| known.kind).can_type();
        let name = match (name, found) {
            ("", Some(known)) => known.name.as_str(),
            (name, _) => name,
        };
        Ok(Step::Paired {
            id: device,
            typing,
            name: paperpaw_ui_kit::text::copy(name)?,
        })
    }

    fn adding_id(&self) -> Option<u32> {
        match self.adding {
            Some(Adding::Connecting(id) | Adding::Pairing(id)) => Some(id),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;

    fn bond(id: u32, kind: Kind, keys: bool) -> Bond {
        Bond {
            id,
            name: String::from("x"),
            kind,
            connected: false,
            keys,
            given: String::new(),
        }
    }

    fn found(id: u32, signal: Option<i32>, kind: Kind) -> Found {
        Found {
            id,
            name: String::from("d"),
            kind,
            signal,
            bonded: false,
        }
    }

    #[test]
    fn rows_give_a_sensor_one_row_per_plugin_and_a_remote_none() {
        let bonds = [bond(1, Kind::Remote, false), bond(2, Kind::Sensor, false)];
        let users = [
            User {
                id: "a".into(),
                name: "A".into(),
            },
            User {
                id: "b".into(),
                name: "B".into(),
            },
        ];
        assert_eq!(
            rows(&bonds, &users).unwrap(),
            [
                Row::Device(0),
                Row::Device(1),
                Row::Give(1, 0),
                Row::Give(1, 1),
                Row::Add,
                Row::Remotes
            ]
        );
        assert!(wants_users(&bonds));
        assert!(!wants_users(&bonds[..1]));
    }

    #[test]
    fn remotes_policy_cycles_and_unknown_is_by_hand() {
        assert_eq!(Policy::from_key("nope"), Policy::ByHand);
        let mut policy = Policy::ByHand;
        for _ in 0..3 {
            policy = Policy::from_key(policy.next());
        }
        assert_eq!(policy, Policy::ByHand);
        assert_eq!(Policy::ByHand.next(), WHILE_READING);
        assert_eq!(Policy::WhileAwake.next(), BY_HAND);
    }

    #[test]
    fn remotes_connect_by_hand_never_by_themselves() {
        for edge in [Edge::BookOpened, Edge::BookClosed] {
            assert_eq!(plan(Policy::ByHand, edge, true), Plan::Nothing);
        }
        assert_eq!(plan(Policy::ByHand, Edge::Awake, false), Plan::SwitchedOn);
        let on = bond(1, Kind::Remote, true);
        let off = bond(2, Kind::Keyboard, false);
        assert_eq!(switched(Plan::SwitchedOn, &on), Some(true));
        assert_eq!(switched(Plan::SwitchedOn, &off), None);
    }

    #[test]
    fn remotes_connect_while_reading_a_book() {
        let p = Policy::WhileReading;
        assert_eq!(plan(p, Edge::BookOpened, true), Plan::AllOn);
        assert_eq!(plan(p, Edge::BookClosed, false), Plan::AllOff);
        assert_eq!(plan(p, Edge::Awake, true), Plan::AllOn);
        assert_eq!(plan(p, Edge::Awake, false), Plan::Nothing);
    }

    #[test]
    fn remotes_connect_whenever_awake_and_only_those_that_type() {
        assert_eq!(plan(Policy::WhileAwake, Edge::Awake, false), Plan::AllOn);
        assert_eq!(
            plan(Policy::WhileAwake, Edge::BookOpened, true),
            Plan::Nothing
        );
        assert_eq!(switched(Plan::AllOn, &bond(1, Kind::Sensor, false)), None);
        assert_eq!(
            switched(Plan::AllOff, &bond(1, Kind::Remote, true)),
            Some(false)
        );
    }

    #[test]
    fn a_search_lists_the_strongest_first_and_each_device_once() {
        let mut search = Search::default();
        search.found(found(1, Some(-80), Kind::Remote)).unwrap();
        search.found(found(2, Some(-40), Kind::Remote)).unwrap();
        search.found(found(3, None, Kind::Remote)).unwrap();
        search.found(found(1, Some(-30), Kind::Remote)).unwrap();
        let order: Vec<u32> = search.found.iter().map(|f| f.id).collect();
        assert_eq!(order, [1, 2, 3]);
        assert_eq!(search.len(), 4);
    }

    #[test]
    fn adding_connects_then_pairs_then_reports() {
        let mut search = Search::default();
        search.found(found(7, Some(-50), Kind::Remote)).unwrap();
        assert_eq!(search.choose(0), Choice::Add(7));
        // Nothing changes until the command is queued.
        assert_eq!(search.adding, None);
        search.begin_add(7);
        // Nothing else is chosen while one is being added.
        assert_eq!(search.choose(0), Choice::Nothing);
        assert_eq!(search.connected(8), Step::Nothing);
        assert_eq!(search.connected(7), Step::Pair(7));
        assert_eq!(
            search.paired(7, Kind::Other, "").unwrap(),
            Step::Paired {
                id: 7,
                typing: true,
                name: "d".into()
            }
        );
        assert_eq!(search.adding, None);
        assert_eq!(search.choose(1), Choice::Search);
    }

    #[test]
    fn a_declined_pairing_and_a_lost_link_say_why() {
        let mut search = Search::default();
        search.found(found(7, None, Kind::Sensor)).unwrap();
        search.begin_add(7);
        search.failed(7, Some(BluetoothReason::Declined));
        assert_eq!(search.problem, Some(Problem::Refused));
        assert_eq!(search.adding, None);
        search.begin_add(7);
        search.lost(7, BluetoothReason::Lost);
        assert_eq!(search.problem, Some(Problem::Failed));
        // A failure of another device or a link nobody is adding changes nothing.
        search.problem = None;
        search.lost(9, BluetoothReason::Lost);
        search.failed(9, None);
        assert_eq!(search.problem, None);
    }

    #[test]
    fn a_failed_search_says_when_the_radio_is_off() {
        let mut search = Search::default();
        search.start();
        search.failed(0, Some(BluetoothReason::Off));
        assert!(!search.searching);
        assert_eq!(search.problem, Some(Problem::Off));
        search.start();
        assert_eq!(search.problem, None);
        search.failed(0, None);
        assert_eq!(search.problem, Some(Problem::Failed));
    }

    #[test]
    fn a_notice_that_fails_changes_nothing() {
        for error in [
            Error::NoMemory,
            Error::NoSpace,
            Error::State,
            Error::Invalid,
        ] {
            best_effort::<()>(Err(error));
        }
        best_effort(Ok(()));
        // What the notice is about is decided before it: pairing sends the keys,
        // the disconnect and back, then the notice; forgetting forgets, then says so.
        let mut search = Search::default();
        search.found(found(7, None, Kind::Remote)).unwrap();
        search.begin_add(7);
        let Step::Paired { id, typing, .. } = search.paired(7, Kind::Other, "").unwrap() else {
            panic!("paired");
        };
        assert_eq!((id, typing, search.adding), (7, true, None));
    }

    #[test]
    fn host_kinds_map_and_unknown_ones_are_other() {
        for (wire, kind) in [
            (1, Kind::Keyboard),
            (2, Kind::Remote),
            (3, Kind::Sensor),
            (4, Kind::Phone),
            (0, Kind::Other),
            (99, Kind::Other),
        ] {
            assert_eq!(Kind::from_wire(wire), kind);
        }
        assert!(Kind::Keyboard.can_type() && Kind::Remote.can_type());
        assert!(!Kind::Sensor.can_type() && !Kind::Phone.can_type());
    }
}
