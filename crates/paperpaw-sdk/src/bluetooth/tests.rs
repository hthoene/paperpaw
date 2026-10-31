use super::*;

#[test]
fn uuids_of_the_sig_and_in_full() {
    let hr = Uuid::sig(0x180D);
    assert_eq!(hr.to_string(), "0000180d-0000-1000-8000-00805f9b34fb");
    assert_eq!(hr.short(), Some(0x180D));
    assert_eq!(Uuid::parse("0x180D"), Some(hr));
    assert_eq!(Uuid::parse("180d"), Some(hr));
    assert_eq!(Uuid::parse(&hr.to_string()), Some(hr));
    let own = Uuid::parse("6e400001-b5a3-f393-e0a9-e50e24dcca9e").unwrap();
    assert_eq!(own.short(), None);
    assert_eq!(own.to_string(), "6e400001-b5a3-f393-e0a9-e50e24dcca9e");
    assert_eq!(Uuid::parse("6e400001b5a3f393e0a9e50e24dcca9e"), None);
    assert_eq!(Uuid::parse("nope"), None);
}

/// Written out for observers, an event says what happened and never
/// carries its payload; the plugin it is for gets it whole.
#[test]
fn events_written_out_carry_no_payload() {
    let device = DeviceId::new(7);
    let value = BtEvent::Value {
        device,
        handle: Handle::new(0x2a),
        kind: ValueKind::Notified,
        bytes: alloc::vec![0, 72],
    };
    let written = value.to_params();
    assert_eq!(written.get(bt_params::LENGTH), Some("2"));
    assert!(
        written.iter().all(|(_, v)| !v.contains("48")),
        "{written:?}"
    );
    assert_eq!(BtEvent::from_params(&written), None);
    let carried = value.clone().into_params();
    assert_eq!(carried.carried::<BtEvent>(), Some(&value));
}

#[test]
fn events_without_a_payload_are_read_back() {
    let device = DeviceId::new(7);
    let handle = Handle::new(3);
    for event in [
        BtEvent::ScanEnded,
        BtEvent::Disconnected {
            device,
            reason: Some(BtError::Lost),
        },
        BtEvent::ServicesChanged { device },
        BtEvent::Written { device, handle },
        BtEvent::Sent { device, handle },
        BtEvent::Paired { device },
        BtEvent::Done {
            op: BtOp::Forget,
            device,
        },
        BtEvent::Failed(
            BtFailure::new(BtOp::Read, BtError::Att(AttError::READ_NOT_PERMITTED))
                .device(device)
                .handle(handle),
        ),
        BtEvent::Advertising,
        BtEvent::AdvertisingEnded,
    ] {
        let written = event.to_params();
        assert_eq!(BtEvent::from_params(&written), Some(event), "{written:?}");
    }
}

#[test]
fn scans_filter_and_are_time_bound() {
    let mut found = Device::new(DeviceId::new(1));
    found.name = Some("Remote 2".into());
    found.services.push(Uuid::sig(0x1812));
    found
        .manufacturer_data
        .push(ManufacturerData::new(0x004c, [1]));
    let events: Event<BtEvent> = Event::new("a.found");
    assert!(scan(&events).passes(&found));
    assert!(scan(&events).service(Uuid::sig(0x1812)).passes(&found));
    assert!(!scan(&events).service(Uuid::sig(0x180D)).passes(&found));
    assert!(scan(&events).name_prefix("Remote").passes(&found));
    assert!(!scan(&events).name_prefix("Pulse").passes(&found));
    assert!(scan(&events).manufacturer(0x004c).passes(&found));
    assert!(!scan(&events).manufacturer(0x0006).passes(&found));
    // Cheap and short unless asked; never long.
    let default = scan(&events);
    assert_eq!(default.power, ScanPower::Low);
    assert!(!default.active);
    assert_eq!(scan(&events).lasting(Duration::MAX).lasting, SCAN_MOST);
    let fast = scan(&events)
        .lasting(Duration::from_secs(50))
        .power(ScanPower::Fast);
    assert_eq!(fast.lasting, FAST_SCAN_MOST);
}

#[test]
fn links_and_advertising_are_cheap_by_default() {
    let events: Event<BtEvent> = Event::new("a.link");
    assert_eq!(connect(DeviceId::new(1), &events).power, LinkPower::Low);
    let seen = advertise(&events);
    assert_eq!(seen.power, AdvertisePower::Low);
    assert!(!seen.connectable);
    assert_eq!(
        advertise(&events).lasting(Duration::MAX).lasting,
        ADVERTISE_MOST
    );
}

#[test]
fn properties_as_the_declaration_has_them() {
    let p = Properties::READ | Properties::NOTIFY;
    assert_eq!(p.bits(), 0x12);
    assert!(p.contains(Properties::READ));
    assert!(!p.contains(Properties::WRITE));
    let spec = CharacteristicSpec::new(Uuid::sig(0x2A37)).read().notify();
    assert_eq!(spec.properties, p);
    assert_eq!(spec.access, Access::Encrypted);
}

#[test]
fn errors_by_name() {
    for error in [
        BtError::Off,
        BtError::NotFound,
        BtError::Declined,
        BtError::Att(AttError::INSUFFICIENT_ENCRYPTION),
        BtError::Other(300),
    ] {
        assert_eq!(BtError::from_param(&error.to_param()), Some(error));
    }
    assert_eq!(DeviceId::from_param("12"), Some(DeviceId::new(12)));
}

struct Only(&'static str);

impl crate::secrets::Running for Only {
    fn runs(&self, plugin: &str) -> bool {
        plugin == self.0
    }
}

/// A plugin sees the bonded devices given to it; the manager sees all,
/// and who each is given to.
#[test]
fn each_plugin_sees_its_own_bonds() {
    let mut all = Bluetooth::new(2);
    all.set_bonded(alloc::vec![
        Bonded::new(DeviceId::new(1), "Belt", DeviceKind::Sensor)
            .given_to(alloc::vec![Id::new_static("pulse")]),
        Bonded::new(DeviceId::new(2), "Remote", DeviceKind::Remote),
    ]);
    all.set_manager(Some(Id::new_static("bluetooth")));
    let pulse = Only("pulse");
    let view = BluetoothView::new(&all, &pulse);
    assert_eq!(view.bonded().count(), 1);
    assert!(view.bond(DeviceId::new(2)).is_none());
    assert!(view.given(DeviceId::new(1)).is_empty());
    let manager = Only("bluetooth");
    let view = BluetoothView::new(&all, &manager);
    assert!(view.manager());
    assert_eq!(view.bonded().count(), 2);
    assert_eq!(view.given(DeviceId::new(1)), [Id::new_static("pulse")]);
}
