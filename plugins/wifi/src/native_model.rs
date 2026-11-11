//! What the native Wi-Fi page and the page of the networks show and decide,
//! host-free: the rows, the words about a network and what choosing one
//! does. The rules of the interface-6 `page.rs` and `networks.rs`; every
//! allocation is fallible.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{mem::push, text::copy};

use crate::declarations::IDLE_OFF;

/// A row of the Wi-Fi page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PageRow {
    /// The radio's switch, with where it is.
    Radio,
    /// Setting up with a phone.
    Phone,
    /// The page of the networks in reach.
    Networks,
    /// The radio comes on by itself when a plugin wants the network.
    OnDemand,
    /// How long after the last use the radio goes off.
    IdleOff,
    /// The radio stays on while charging.
    WhileCharging,
}

/// A vector with room for exactly `count` items (`push` would take room for
/// four at the first one).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn room<T>(count: usize) -> Result<Vec<T>, Error> {
    let mut items = Vec::new();
    items
        .try_reserve_exact(count)
        .map_err(|_| Error::NoMemory)?;
    Ok(items)
}

/// A vector of the one `item`, with room for exactly it.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn one<T>(item: T) -> Result<Vec<T>, Error> {
    let mut items = room(1)?;
    items.push(item);
    Ok(items)
}

/// The rows of the Wi-Fi page: where no network is kept the setup with a
/// phone comes first, below the radio; else the networks (while the radio
/// is on) and then the setup.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn page_rows(kept: bool, radio_on: bool) -> Result<Vec<PageRow>, Error> {
    // The radio, the phone, the three settings, and the networks while on.
    let mut rows = room(5 + usize::from(radio_on))?;
    rows.push(PageRow::Radio);
    if !kept {
        rows.push(PageRow::Phone);
    }
    if radio_on {
        rows.push(PageRow::Networks);
    }
    if kept {
        rows.push(PageRow::Phone);
    }
    rows.extend([PageRow::OnDemand, PageRow::IdleOff, PageRow::WhileCharging]);
    Ok(rows)
}

/// The choice after `now` in the idle choices (the first after the last).
#[must_use]
pub fn next_idle(now: &str) -> &'static str {
    let at = IDLE_OFF
        .iter()
        .position(|choice| *choice == now)
        .unwrap_or(0);
    IDLE_OFF[(at + 1) % IDLE_OFF.len()]
}

/// A network in reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Network {
    pub ssid: String,
    /// Signal strength (dBm).
    pub signal: i32,
    /// It asks for a password.
    pub secured: bool,
}

impl Network {
    /// A copy of the network.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn try_clone(&self) -> Result<Self, Error> {
        Ok(Self {
            ssid: copy(&self.ssid)?,
            signal: self.signal,
            secured: self.secured,
        })
    }
}

/// The signal in bars from 1 to 4.
#[must_use]
pub fn bars(signal: i32) -> u8 {
    match signal {
        s if s >= -55 => 4,
        s if s >= -67 => 3,
        s if s >= -78 => 2,
        _ => 1,
    }
}

/// The words for a signal.
#[must_use]
pub fn signal_key(signal: i32) -> &'static str {
    match bars(signal) {
        4 => "wifi.signal.excellent",
        3 => "wifi.signal.good",
        2 => "wifi.signal.fair",
        _ => "wifi.signal.weak",
    }
}

/// Where a network stands for this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    /// The device is in it.
    Connected,
    /// The device is joining it.
    Connecting,
    /// Neither.
    Apart,
}

/// What the note of a network says, as the keys of its words: joined or
/// saved, its signal, open or not.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn note_keys(link: Link, saved: bool, network: &Network) -> Result<Vec<&'static str>, Error> {
    // At most three keys: the room is exact.
    let mut keys = room(3)?;
    match link {
        Link::Connected => keys.push("wifi.network.connected"),
        Link::Connecting => keys.push("wifi.network.connecting"),
        Link::Apart if saved => keys.push("wifi.network.saved"),
        Link::Apart => {}
    }
    keys.push(signal_key(network.signal));
    if !network.secured {
        keys.push("wifi.network.open");
    }
    Ok(keys)
}

/// What choosing a network does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// The device is in it or joining it: nothing.
    Nothing,
    /// A network kept: joined with the password kept.
    Kept,
    /// A secured network not kept: its password is asked for.
    Ask,
    /// An open network not kept: joined once, not by itself again.
    Open,
}

/// What choosing `network` does, the device being in (or joining) the
/// network `at` and `saved` telling whether the network is kept.
#[must_use]
pub fn pick(at: Option<&str>, saved: bool, network: &Network) -> Pick {
    if at == Some(network.ssid.as_str()) {
        Pick::Nothing
    } else if saved {
        Pick::Kept
    } else if network.secured {
        Pick::Ask
    } else {
        Pick::Open
    }
}

/// Whether `ssid` is among the `names` of the networks kept.
#[must_use]
pub fn is_kept(names: &[String], ssid: &str) -> bool {
    names.iter().any(|name| name == ssid)
}

/// The networks the device found, strongest first, as the host lists them,
/// at most `most` (the rest are not read).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn collect<'a>(
    found: impl IntoIterator<Item = (&'a str, i32, bool)>,
    most: usize,
) -> Result<Vec<Network>, Error> {
    let mut networks = Vec::new();
    for (ssid, signal, secured) in found {
        if networks.len() == most {
            break;
        }
        push(
            &mut networks,
            Network {
                ssid: copy(ssid)?,
                signal,
                secured,
            },
        )?;
    }
    Ok(networks)
}

/// A notice is best effort: the work it reports is done all the same.
pub fn best_effort(result: Result<(), Error>) {
    let _ = result;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(ssid: &str, signal: i32, secured: bool) -> Network {
        Network {
            ssid: ssid.into(),
            signal,
            secured,
        }
    }

    #[test]
    fn the_phone_comes_first_while_no_network_is_kept() {
        use PageRow::{IdleOff, Networks, OnDemand, Phone, Radio, WhileCharging};
        assert_eq!(
            page_rows(false, true).unwrap(),
            [Radio, Phone, Networks, OnDemand, IdleOff, WhileCharging]
        );
        assert_eq!(
            page_rows(true, true).unwrap(),
            [Radio, Networks, Phone, OnDemand, IdleOff, WhileCharging]
        );
        assert_eq!(
            page_rows(true, false).unwrap(),
            [Radio, Phone, OnDemand, IdleOff, WhileCharging]
        );
    }

    #[test]
    fn rooms_are_exact() {
        assert_eq!(one(7u8).unwrap().capacity(), 1);
        assert_eq!(room::<u8>(0).unwrap().capacity(), 0);
        assert_eq!(page_rows(true, true).unwrap().capacity(), 6);
        assert_eq!(page_rows(true, false).unwrap().capacity(), 5);
        let keys = note_keys(Link::Apart, false, &net("a", -50, true)).unwrap();
        assert_eq!(keys.len(), 1);
        assert!(keys.capacity() <= 3);
    }

    #[test]
    fn the_idle_choices_go_round() {
        assert_eq!(next_idle("wifi.idle-off.1m"), "wifi.idle-off.2m");
        assert_eq!(next_idle("wifi.idle-off.10m"), "wifi.idle-off.30s");
        // Unknown: the one after the first.
        assert_eq!(next_idle("x"), "wifi.idle-off.1m");
    }

    #[test]
    fn bars_follow_the_sdks() {
        for signal in i8::MIN..=0 {
            let old = paperpaw_sdk::wifi::WifiNetwork::new("x", signal, true);
            assert_eq!(bars(i32::from(signal)), old.bars(), "{signal}");
        }
    }

    #[test]
    fn notes_name_the_link_the_signal_and_openness() {
        let n = net("a", -50, false);
        assert_eq!(
            note_keys(Link::Connected, true, &n).unwrap(),
            [
                "wifi.network.connected",
                "wifi.signal.excellent",
                "wifi.network.open"
            ]
        );
        assert_eq!(
            note_keys(Link::Apart, true, &net("a", -90, true)).unwrap(),
            ["wifi.network.saved", "wifi.signal.weak"]
        );
        assert_eq!(
            note_keys(Link::Connecting, false, &net("a", -70, true)).unwrap(),
            ["wifi.network.connecting", "wifi.signal.fair"]
        );
    }

    #[test]
    fn choosing_decides_by_the_link_the_kept_password_and_openness() {
        let secured = net("a", -50, true);
        let open = net("b", -50, false);
        assert_eq!(pick(Some("a"), true, &secured), Pick::Nothing);
        assert_eq!(pick(None, true, &secured), Pick::Kept);
        assert_eq!(pick(Some("c"), false, &secured), Pick::Ask);
        assert_eq!(pick(None, false, &open), Pick::Open);
        // A kept open network is joined as kept.
        assert_eq!(pick(None, true, &open), Pick::Kept);
    }

    #[test]
    fn a_copy_is_equal() {
        let n = net("a", -50, true);
        assert_eq!(n.try_clone().unwrap(), n);
    }

    #[test]
    fn at_most_the_wanted_networks_are_read() {
        let all = [("a", -40, true), ("b", -50, false), ("c", -60, true)];
        assert_eq!(collect(all, 2).unwrap().len(), 2);
        assert_eq!(collect(all, 9).unwrap()[2].ssid, "c");
        assert!(is_kept(&["a".into()], "a"));
        assert!(!is_kept(&["a".into()], "b"));
    }
}
