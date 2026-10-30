//! The device's name, as others see it: the computer Calibre runs on, the
//! progress server, the router's list of devices, a phone joining the
//! reader's own network. The reader sets it (*Device name*, [`SETTING`]);
//! plugins take it from [`Env::device_name`] and never make one of their
//! own. Changed, it counts from the next time something connects.
//!
//! [`Env::device_name`]: crate::Env::device_name

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::String;

use paperpaw_platform::DeviceInfo;

use crate::setting::{Setting, Text};
use crate::{SettingKind, SettingSpec, SettingValue};

/// The setting of the name, under System (the kernel's).
pub const SETTING: Setting<Text> = Setting::new("core.device-name");

/// The setting as the kernel offers it, `default` (the model's,
/// [`default_name`]) until the reader sets another.
pub fn setting(default: String) -> SettingSpec {
    SettingSpec::new(
        crate::Id::new_static(SETTING.static_str()),
        "core.setting.device-name",
        SettingKind::Text,
        SettingValue::Text(Cow::Owned(default)),
        // The Wi-Fi plugin's: the name others see the device by.
        crate::Id::new_static("wifi.group"),
    )
    .description("core.setting.device-name.help")
}

/// Characters of a name at most.
pub const MAX_CHARS: usize = 40;
/// Characters of a host name at most (as a router lists it).
pub const MAX_HOST: usize = 32;
/// Bytes of a network's name at most (IEEE 802.11).
pub const MAX_SSID: usize = 32;
/// What a name with nothing usable in it becomes.
const FALLBACK: &str = "PaperPaw";

/// The name a device has until the reader gives it another: on a device
/// PaperPaw is the system of (an Xteink), "PaperPaw" and its model
/// ("PaperPaw X4 Pro"); elsewhere the model ("Boox Go 7").
pub fn default_name(device: &DeviceInfo) -> String {
    match device.name.strip_prefix("Xteink ") {
        Some(model) => format!("PaperPaw {model}"),
        None => String::from(device.name),
    }
}

/// The name set as `value` (the setting's), without what shows nothing
/// and at most [`MAX_CHARS`] characters; the [`default_name`] where
/// nothing is left.
pub fn chosen(value: &str, device: &DeviceInfo) -> String {
    let shown: String = value
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_CHARS)
        .collect();
    match shown.trim() {
        "" => default_name(device),
        name => String::from(name),
    }
}

/// `name` as a host name for the router (DHCP's option 12): letters and
/// digits, a hyphen for each run of anything else, apostrophes left out,
/// common accented letters spelled without their marks (ä as ae);
/// [`MAX_HOST`] characters at most, no hyphen at either end.
/// "Hannes’ Reader" is `Hannes-Reader`.
pub fn host_name(name: &str) -> String {
    let mut out = String::new();
    let mut apart = false;
    for c in name.chars() {
        if matches!(c, '\'' | '’' | 'ʼ' | '‘' | '`' | '´') {
            continue;
        }
        let plain = plain(c);
        if plain.is_empty() {
            apart = !out.is_empty();
            continue;
        }
        if apart {
            out.push('-');
            apart = false;
        }
        out.push_str(&plain);
    }
    // Cut whole: every character is one byte now.
    out.truncate(MAX_HOST);
    let host = out.trim_end_matches('-');
    if host.is_empty() {
        String::from(FALLBACK)
    } else {
        String::from(host)
    }
}

/// `c` in plain letters and digits: itself, a common accented letter
/// without its marks (ä as ae, ß as ss, in its case), else nothing.
fn plain(c: char) -> String {
    if c.is_ascii_alphanumeric() {
        return String::from(c);
    }
    let lower = c.to_lowercase().next().unwrap_or(c);
    let spelled = match lower {
        'ä' | 'æ' => "ae",
        'ö' | 'œ' => "oe",
        'ü' => "ue",
        'ß' => "ss",
        'à' | 'á' | 'â' | 'ã' | 'å' | 'ā' | 'ą' => "a",
        'ç' | 'ć' | 'č' => "c",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => "e",
        'ì' | 'í' | 'î' | 'ï' | 'ī' | 'į' | 'ı' => "i",
        'ñ' | 'ń' | 'ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ø' | 'ō' | 'ő' => "o",
        'ù' | 'ú' | 'û' | 'ū' | 'ů' | 'ű' => "u",
        'ý' | 'ÿ' => "y",
        'ł' => "l",
        'ś' | 'š' | 'ş' => "s",
        'ź' | 'ż' | 'ž' => "z",
        'ř' => "r",
        'ď' => "d",
        'ť' => "t",
        _ => return String::new(),
    };
    if c == lower {
        String::from(spelled)
    } else {
        // A capital: its first letter a capital (Ä as Ae).
        let mut chars = spelled.chars();
        let first = chars.next().map(|f| f.to_ascii_uppercase());
        first.into_iter().chain(chars).collect()
    }
}

/// `name` as the name of a network the device opens (for a phone to
/// join): as it is, cut at [`MAX_SSID`] bytes on a character's edge.
pub fn ssid(name: &str) -> String {
    let mut end = name.len().min(MAX_SSID);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    match name[..end].trim() {
        "" => String::from(FALLBACK),
        ssid => String::from(ssid),
    }
}

#[cfg(test)]
mod tests {
    use paperpaw_platform::{DisplayInfo, InputCaps, MemoryClass, PixelFormat, Rotation};

    use super::*;

    fn device(name: &'static str) -> DeviceInfo {
        DeviceInfo::new(
            "test",
            name,
            DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
            InputCaps::new(true, false, false, &[]),
            MemoryClass::Small,
        )
    }

    #[test]
    fn names_default_to_the_model() {
        assert_eq!(default_name(&device("Xteink X4 Pro")), "PaperPaw X4 Pro");
        assert_eq!(default_name(&device("Xteink X4")), "PaperPaw X4");
        assert_eq!(default_name(&device("Boox Go 7")), "Boox Go 7");
        let x4 = device("Xteink X4");
        assert_eq!(chosen("", &x4), "PaperPaw X4");
        assert_eq!(chosen("  \t ", &x4), "PaperPaw X4");
        assert_eq!(chosen(" Hannes’ Reader \n", &x4), "Hannes’ Reader");
        assert_eq!(chosen(&"x".repeat(60), &x4).len(), MAX_CHARS);
    }

    #[test]
    fn host_names_keep_letters_digits_and_hyphens() {
        assert_eq!(host_name("Hannes’ Reader"), "Hannes-Reader");
        assert_eq!(host_name("PaperPaw X4 Pro"), "PaperPaw-X4-Pro");
        assert_eq!(host_name("Jörg's Böx #2"), "Joergs-Boex-2");
        assert_eq!(host_name("  --Ärger!!  "), "Aerger");
        assert_eq!(host_name("日本語"), "PaperPaw");
        assert_eq!(host_name(""), "PaperPaw");
        let long = host_name(&"abc ".repeat(20));
        assert!(long.len() <= MAX_HOST && !long.ends_with('-'), "{long}");
        for name in ["Hannes’ Reader", "a b", "Ünïcödé ßtraße", "x--y"] {
            let host = host_name(name);
            assert!(
                host.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                    && !host.starts_with('-')
                    && !host.ends_with('-')
                    && !host.contains("--"),
                "{name}: {host}"
            );
        }
    }

    #[test]
    fn network_names_are_cut_on_a_character() {
        assert_eq!(ssid("PaperPaw X4"), "PaperPaw X4");
        let long = ssid(&"ä".repeat(20));
        assert!(long.len() <= MAX_SSID && long.chars().all(|c| c == 'ä'));
        assert_eq!(ssid(" "), "PaperPaw");
    }
}
