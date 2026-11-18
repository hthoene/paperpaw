#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::*;
use crate::abi7::settings_wire::settings_decode;
use alloc::string::String;
use paperpaw_platform::Battery;

#[test]
fn old_values_are_typed_and_over_cap_values_are_dropped_not_cut() {
    assert!(OldValue::capture(None).bytes().unwrap().is_empty());
    for value in [
        SettingValue::Bool(false),
        SettingValue::Int(i64::MIN),
        SettingValue::Text(String::new().into()),
    ] {
        let captured = OldValue::capture(Some(&value));
        let bytes = captured.bytes().unwrap();
        assert!(!bytes.is_empty());
        match (value, settings_decode(bytes).unwrap()) {
            (SettingValue::Bool(a), SettingValueRef::Bool(b)) => assert_eq!(a, b),
            (SettingValue::Int(a), SettingValueRef::Int(b)) => assert_eq!(a, b),
            (SettingValue::Text(a), SettingValueRef::Text(b)) => assert_eq!(a, b),
            _ => panic!("changed value kind"),
        }
    }
    let exact = SettingValue::Text("x".repeat(500).into());
    assert_eq!(OldValue::capture(Some(&exact)).bytes().unwrap().len(), 512);
    let over = SettingValue::Text("x".repeat(501).into());
    assert_eq!(OldValue::capture(Some(&over)).bytes(), Err(ERR_NO_SPACE));
}

#[test]
fn wifi_access_point_and_power_keep_the_interface_six_meaning() {
    let ap = WifiState::Hosting {
        ssid: "PaperPaw".into(),
        address: "192.168.4.1".into(),
        guests: 1,
    };
    let power = Power::new(
        PowerLevel::Offline,
        paperpaw_sdk::Id::new_static("preset.mine"),
        true,
        7,
    );
    let mut out = [0xa5; 256];
    let n = Hint::Wifi(&ap).encode(42, &mut out).unwrap();
    let wire::Record::EventWifiChanged(wifi) = wire::records_decode_single(&out[..n]).unwrap()
    else {
        panic!()
    };
    assert_eq!((wifi.now_ms, wifi.state), (42, 4));
    let n = Hint::Power(&power).encode(43, &mut out).unwrap();
    let wire::Record::EventPowerChanged(event) = wire::records_decode_single(&out[..n]).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        (
            event.now_ms,
            event.level,
            event.flags,
            event.stretch,
            event.mode
        ),
        (43, 2, 1, 7, "preset.mine")
    );
}

#[test]
fn charge_uses_known_external_power_and_the_hosts_existing_low_threshold() {
    let empty = SystemStatus::default();
    assert_eq!(ChargeState::from_status(&empty, None).flags(), 0);
    assert_eq!(ChargeState::from_status(&empty, Some(true)).flags(), 2);
    let low = empty.with_battery(Some(Battery::new(10, false, None)));
    assert_eq!(ChargeState::from_status(&low, None).flags(), 4);
    let charging = empty.with_battery(Some(Battery::new(5, true, None)));
    assert_eq!(ChargeState::from_status(&charging, None).flags(), 3);
    let full = empty.with_battery(Some(Battery::new(100, false, None)));
    assert_eq!(ChargeState::from_status(&full, None).flags(), 0);
    assert_eq!(ChargeState::from_status(&full, Some(true)).flags(), 2);
}

#[test]
fn hint_encoding_is_bounded_and_failed_output_is_untouched() {
    let off = WifiState::Off;
    let power = Power::default();
    let charge = ChargeState::from_status(&SystemStatus::default(), None);
    for hint in [
        Hint::Plugins,
        Hint::Wifi(&off),
        Hint::Power(&power),
        Hint::Status,
        Hint::Charge(charge),
    ] {
        let mut full = [0; 256];
        let n = hint.encode(42, &mut full).unwrap();
        assert_eq!(u32::from(full[1]), hint.tag());
        for capacity in 0..n {
            let mut short = [0xa5; 256];
            assert_eq!(hint.encode(42, &mut short[..capacity]), Err(ERR_NO_SPACE));
            assert_eq!(short, [0xa5; 256]);
        }
    }
}
