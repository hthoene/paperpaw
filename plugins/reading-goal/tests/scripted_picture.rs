//! A live widget requests the installed cat through the bundle image root.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]

use paperpaw_core::native_host::{
    commands::{WireCommand, commands_decode},
    environment::{SNAPSHOT_BYTES, SNAPSHOT_VERSION},
    registrations::{EventWidgetMade, Record, tick_encode},
};
use paperpaw_plugin_reading_goal::NATIVE_ENTRY;
use paperpaw_scripted_host::{ScriptedHost, event_batch};

#[test]
fn a_widget_without_custom_pictures_requests_the_bundled_cat() {
    let host = ScriptedHost::new();
    let mut snapshot = vec![0; SNAPSHOT_BYTES];
    snapshot[0..4].copy_from_slice(&SNAPSHOT_VERSION.to_le_bytes());
    snapshot[4..8].copy_from_slice(&u32::try_from(SNAPSHOT_BYTES).unwrap().to_le_bytes());
    snapshot[32..34].copy_from_slice(&480u16.to_le_bytes());
    snapshot[34..36].copy_from_slice(&800u16.to_le_bytes());
    snapshot[36..38].copy_from_slice(&220u16.to_le_bytes());
    snapshot[42] = 1;
    host.set_env(&snapshot);
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    let made = event_batch(&Record::EventWidgetMade(EventWidgetMade {
        now_ms: 0,
        widget: "reading-goal.widget",
        instance: 4,
        place: "",
    }))
    .unwrap();
    assert_eq!(plugin.event(2, &made).unwrap().status, 0);
    let mut tick = [0; 64];
    let n = tick_encode(60_000, &mut tick).unwrap();
    let done = plugin.event(3, &tick[..n]).unwrap();
    assert_eq!(done.status, 0);
    let pictures: Vec<_> = done
        .batches
        .iter()
        .flat_map(|batch| commands_decode(batch).unwrap().iter().collect::<Vec<_>>())
        .filter_map(|command| match command {
            WireCommand::ImagePrepare {
                root, path, side, ..
            } => Some((root, path, side)),
            _ => None,
        })
        .collect();
    assert_eq!(pictures, [(2, "pictures/sad.png", 0)]);
}
