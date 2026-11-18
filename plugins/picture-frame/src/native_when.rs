//! When the server's photos are fetched, decided without the host. Never at a
//! wake and never for a photo alone: only on request, with the charger, and,
//! where the setting says, when Wi-Fi is up anyway (the reader joined it for
//! something else; this plugin does not turn it on).
use crate::declarations::SYNC;

/// What the setting says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// Only when asked.
    Manual,
    /// When the charger is plugged in, at most every six hours.
    Charging,
    /// As with the charger, and when Wi-Fi comes up anyway, at most daily.
    Online,
}

impl Policy {
    /// The setting's value as a policy; with the charger for anything else.
    #[must_use]
    pub fn of(value: &str) -> Self {
        if value == SYNC[0] {
            Self::Manual
        } else if value == SYNC[2] {
            Self::Online
        } else {
            Self::Charging
        }
    }
}

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// The charger was plugged in.
    Charger,
    /// Wi-Fi is connected.
    Network,
}

const SIX_HOURS: i64 = 6 * 3600;
const DAY: i64 = 24 * 3600;

/// Whether to fetch now: `synced` is when it last ended well (0: never) and
/// `now` the time in Unix seconds (`None`: not known, then only a first fetch).
#[must_use]
pub fn due(policy: Policy, trigger: Trigger, synced: i64, now: Option<i64>) -> bool {
    let wait = match (policy, trigger) {
        (Policy::Manual, _) | (Policy::Charging, Trigger::Network) => return false,
        (_, Trigger::Charger) => SIX_HOURS,
        (Policy::Online, Trigger::Network) => DAY,
    };
    match now {
        _ if synced == 0 => true,
        Some(now) => now < synced || now - synced >= wait,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fetch_waits_for_the_charger_or_a_network_that_is_up_anyway() {
        let now = Some(1_000_000);
        // Manual never starts by itself.
        for trigger in [Trigger::Charger, Trigger::Network] {
            assert!(!due(Policy::Manual, trigger, 0, now));
        }
        // With the charger: a first fetch at once, then every six hours.
        assert!(due(Policy::Charging, Trigger::Charger, 0, now));
        assert!(!due(
            Policy::Charging,
            Trigger::Charger,
            1_000_000 - 3600,
            now
        ));
        assert!(due(
            Policy::Charging,
            Trigger::Charger,
            1_000_000 - SIX_HOURS,
            now
        ));
        // Wi-Fi alone does not start the charging policy; the online one, daily.
        assert!(!due(Policy::Charging, Trigger::Network, 0, now));
        assert!(!due(
            Policy::Online,
            Trigger::Network,
            1_000_000 - SIX_HOURS,
            now
        ));
        assert!(due(Policy::Online, Trigger::Network, 1_000_000 - DAY, now));
        // A clock that went back, or none: the first fetch only.
        assert!(due(Policy::Online, Trigger::Network, 2_000_000, now));
        assert!(!due(Policy::Online, Trigger::Network, 5, None));
        assert!(due(Policy::Online, Trigger::Network, 0, None));
    }

    #[test]
    fn the_setting_names_a_policy() {
        assert_eq!(Policy::of(SYNC[0]), Policy::Manual);
        assert_eq!(Policy::of(SYNC[1]), Policy::Charging);
        assert_eq!(Policy::of(SYNC[2]), Policy::Online);
        assert_eq!(Policy::of("nonsense"), Policy::Charging);
    }
}
